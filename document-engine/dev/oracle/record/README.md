# The record

What LibreOffice shows for every Office document the engine's writer produces. `../README.md` says
how the record is produced and what the machinery is; this file says how to read what is here.

**This is a captured artefact, not a sample document.** A sample document is an input the writer is
judged on, and the workspace commits none: every corpus input is synthesised in code. What is here is
an output an outside producer was judged on, kept so the pages can be rasterised and reviewed later.
Nothing in the engine's build or tests reads it; the oracle's own tests read it to prove it is whole.
The comparison reads this record twice: the Office half judges the engine's own rendered pages
against the pages recorded here — page count, page size, every line's text, position and size, every
picture's box and whether a page drew text — and the PDF half judges the engine's own PDF
output against PDFium (`../compare/README.md`).

Reading an entry needs nothing but this directory: no LibreOffice, no network, no build. Reading it
also checks it: the kept PDF must be the one its entry describes — the same object graph, and the
same pages, blank pages and drawn faces — `fonts.conf` must be the one `identity.json` digests and
must name the directories `identity.json` states, and every face a page drew in must be one the
identity states. `../README.md` shows the tree.

## Reading an entry

`document.json` is meant to be read top to bottom.

- `chain` is what produced the document, with `"$previous"` standing for the file the previous
  operation wrote and `"$asset:picture"` for the generated picture. It is path-free, so the entry
  reads the same on any machine.
- `source.digest` is the canonical content of the engine's own output — its parts in sorted order,
  with the two dates the writer stamps left out. If our writer changes, this digest changes and the
  entry is stale: the run fails rather than letting a recorded page stand as if it still described
  today's output.
- `produced.filter` is the export filter, `produced.metadata_excluded` what the digest leaves out,
  and `produced.digest` the digest of the kept file's object graph with the reference's own stamps
  and its font encoding left out (see `../src/canonical.rs` for exactly what, and why).
- `pages.pages` is every page the reference laid the document out into, each with its size in
  points, its lines and its pictures, and the faces it drew a glyph in. Every line states its text,
  its `x_pt` and `y_pt` and its `size_pt`: `x_pt` is the distance from the page's left edge to the
  line's start, `y_pt` the distance from its top edge to the line's baseline, and `size_pt` the size
  the line is drawn at. Every picture states its box the same way: `x_pt` from the left edge to its
  left side, `y_pt` from the top edge to its top side, and `width_pt` and `height_pt` its sides.
  Lines and pictures are in visual order — from the top of the page down, and from the left across —
  so a running header comes first and a footer last, whatever order the content streams draw them
  in. A run on another baseline is another line, which is why a superscript marker stands beside the
  line it marks. `pages.blank` names every page that carries no ink, with the reason it is empty. A
  page that draws nothing lists no face and no line, and a page whose only ink is a running header
  or footer is not blank.
- `faces.named` is what the document itself asks for, and where it says so. `faces.default` is the
  family the pages were actually drawn in when the document names nothing at all — a created Word
  document names nothing, so this is what the reference laid it out in: its own default when the
  machine has that face, its declared metric-compatible substitute when it does not (which
  `faces.metric_compatible` names the pair for), and whatever it fell back to when even that is
  missing (which `faces.not_metric_compatible` names).
  `faces.used` is every face the pages drew a glyph in; `faces.metric_compatible` names the ones that
  are a declared metric-compatible substitute for a face the document asks for (the default included
  when it asks for none), and `faces.not_metric_compatible` the ones that are not, each with the
  reason it is there: the reference fell back to it, or it draws its own glyphs in it.
  `faces.without_metric_substitute` names the families the document asks for that
  the record declares no metric-compatible substitute for — Calibri Light is the one the corpus
  reaches — and `measured_in`, the faces their text can have been measured in instead. A produced
  page names the face that drew a run, not the family that asked for it, and one face can serve more
  than one family, so the record states the candidates rather than guessing the assignment.
  `faces.used` and `measured_in` are the names the produced PDF spells (PostScript names); the
  `face` of a named, substitute or fallback entry is a family name. `identity.json`'s
  `fonts.faces` states both spellings for every face a page drew in, with the digest of its file.
- `engine_computed` names the numbers the document prints from the engine's own layout pass rather
  than from the reference. A table of contents' page numbers and a `{pages}` token's count are ours,
  not the reference's, so a recorded document that looks self-inconsistent about them is showing
  exactly that and nothing more.
- `not_shown` is what the reference's export leaves out, declared where the corpus writes the
  document (`../src/corpus.rs`) and stated per document rather than per family, because the export
  draws what a printed page would show and nothing else. Its silence about a thing is never a
  mandate about that thing.

## The faces

`produced.pdf` carries the font subsets the reference embedded in it, as any PDF that embeds a face
does — in the committed record, subsets of Times New Roman, Carlito and OpenSymbol. They are kept
because the pages must be rasterisable and the record must stand without the reference, and they are
part of the artefact as produced rather than a font distributed here: no font file is committed on
its own, and the faces themselves stay with their own distributions under their own licences.
`identity.json` names each face, the file it came from and that file's digest, so the same faces can
be identified without being shipped, and a verification refuses when a face is not the one recorded —
a newer release of a substitute face would silently move every line break.

`identity.json` also names the reading that grouped the recorded lines and the band it grouped them
by — the reading's own source digest and `line_band_pt`. The digest covers both sources that decide a
recorded coordinate: `../src/positions.rs`, the walk, then `../src/pages.rs`, the page box and content
streams it walks. A change to either changes what the recorded lines mean, so verification refuses
the record until it is re-recorded.

## Re-recording

`DOCUMENT_ENGINE_RECORD=1 cargo test -p document-engine --test oracle -- --ignored --exact record`
re-records the whole corpus; without the variable the mode refuses, because `--ignored` alone reaches
every hidden mode. It surveys twice, refuses unless the second run reproduces the first's pages,
writes this directory and prints what it removed because the corpus no longer declares it. Review the
diff: every changed page is a line of `document.json`.

An entry the corpus no longer declares is **stale and fails the run**: it is never silently kept or
dropped.

An entry whose description is unchanged keeps the `produced.pdf` already on disk, once that file has
been re-checked against the entry — its digest and its pages — so a re-record against an unchanged
writer leaves the tree byte-identical and a corrupted kept document is rewritten rather than trusted.
The kept bytes are the artefact the description binds, and the exporter stamps its own dates and ids
into them, so they are not a change to review. Measured on the machine the record was taken on, a
second recording rewrote nothing. When a description *does* change, that entry's PDF is written anew
— its bytes differ even where its pages do not — and the diff to review is `document.json`; the
`canonical_pdf` mode (`-- --ignored --exact canonical_pdf -- <old> <new>`) says which object of the
kept PDF moved when `produced.digest` does.


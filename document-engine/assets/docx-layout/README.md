# `docx` layout data

The layout tables of the npm package `docx` **9.12.0** — the version `assets/docgen/package.json`
pins — derived from its `dist/layout.mjs` bundle for the Rust layout port to read as data. The data
is not hand-written: to change it, re-derive it. The probes beside it are hand-written scripts that
report what a created document reaches.

## Derivation

    bun parity/scripts/derive-docx-layout-data.mjs

The script takes the package from `$PARITY_DOCX` when it is set (a path to the package directory),
else `~/.bun/install/cache/docx@9.12.0@@@1`, else `node_modules/docx` under the repository, and
stops with the paths it tried when none of them holds `dist/layout.mjs`. It copies the bundle to a
temporary file, appends the `export { … }` statement the module-scope tables need, imports it, and
writes each file with `JSON.stringify(…, null, 2)` and a trailing newline. Every binding, every
`//#region` marker and every constant is checked before the first file is written, so a bundle that
has moved on fails without leaving a partial data set. Two runs over the same package produce
byte-identical files.

## Files

### `widths.json`

Read by the layout port's measurer (`document-engine/src/writing/docx/layout/text.rs`).

| Field | Shape |
| --- | --- |
| `ranges` | `[[first, last], …]` code point ranges of the core width tables (2127 code points) |
| `more_ranges` | `[[first, last], …]` code point ranges of the supplementary width tables |
| `fallback_faces` | `{font, bold, italic}` — the faces Word draws characters in that other fonts lack |
| `faces` | `{name, line_height, descent, regular, bold, italic?, bold_italic?}` |
| `same_widths` | `{pattern, flags, face}`, in match order |
| `similar_fonts` | `{pattern, flags, face}`, in match order |

A face's width strings are the bundle's own encoding, copied verbatim: base-64 digits, `*` for a
repetition and `!` for an unknown width. `italic` and `bold_italic` are absent when the JS face does
not state them (Trebuchet MS states no bold italic; Tahoma and Impact state neither). The width faces
state neither `ligatures` nor `not_kerned_with_ligatures` in this version, so no face carries them;
both are copied if a face states them. `same_widths` and `similar_fonts` are ordered rules — the
first match wins — so they stay an array; `pattern` and `flags` are the regex's `source` and `flags`.
Every alias in this version states `i`, and the port matches by lowercasing the face's name, which is
what `i` asks for: it applies no other flag and refuses a data set that states one rather than measure
with a rule it does not honour.

### `more_widths.json`

Read by the layout port's measurer (`document-engine/src/writing/docx/layout/text.rs`).

`faces` holds `{name, classes}`, each class present only when stated and holding `{widths, fonts}`:
the encoded widths of the supplementary characters, and one token per character naming the face it
is drawn in — an index into `fallback_fonts`, which holds `{name, ascent, descent}` for each face a
supplementary character is drawn in.

### `arabic_widths.json`

Read by the layout port's measurer (`document-engine/src/writing/docx/layout/text.rs`).

| Field | Shape |
| --- | --- |
| `letters` | the Arabic letters the table is for |
| `lam_alefs` | the lam-alef ligature letters |
| `faces` | `{name, regular, bold, drawn_in?}` |

`drawn_in` names the face Word draws the font's Arabic in, when it is not the font itself.

### `constants.json`

Read by the layout port's `constants` module (`document-engine/src/writing/docx/layout/constants.rs`).

An object keyed by constant name, each `{value, quantity, unit}`: the number, what it counts and its
unit. It holds exactly the seven constants the port reads — `PASSES`, `TAB_STOP`, `TOLERANCE`,
`EIGHTHS_PER_POINT`, `FIFTIETHS_OF_A_PERCENT`, `WORD_2007_MODE` and `WORD_2010_MODE` — and nothing
else: a constant nothing reads has no place in the data, and
`document-engine/tests/vocabulary.rs` fails if the data states a name no source reads or a source
reads a name the data does not state. Which module of the bundle states a constant is the bundle's
own business, so the data does not carry it. The bundler's `$1`/`$2` aliases are folded into one
name, and so are two names the bundle gives one quantity: its four `36`s for the distance between
automatic tab stops are stated once, as `TAB_STOP`. The scan reads `quantity` and `unit`: a numeral
that coincides with a vocabulary
bound **of another quantity in another unit** is not a restatement of it, and one that names the same
quantity in the same unit is. (The scan's own exemptions state their quantity the same way, as a field
rather than as prose, so a rewording cannot pass the rule.)

### `unicode.json`

Read by the layout port's line-break rules (`document-engine/src/writing/docx/layout/breaks.rs`).

| Field | Shape |
| --- | --- |
| `marks` | `[[first, last], …]` code points of `\p{M}`, a mark that belongs to the character before it |
| `east_asian` | `[[first, last], …]` code points of the reference's `EAST_ASIAN` class |
| `hangul` | `[[first, last], …]` code points of its `HANGUL` class |

The three regular expressions the reference evaluates are enumerated here rather than restated in
Rust, so the port cannot drift from the expression the pinned library runs.

### `east_asian.json`

Read by the layout port's measurer (`document-engine/src/writing/docx/layout/text.rs`).

`faces` holds the 21 East Asian faces the reference measures Chinese, Japanese and Korean text in,
each `{name, aliases, lineHeight, descent, monospaced, latin}`: the face's own line metrics in
thousandths of an em, whether its characters are an em or half an em wide, and the face its Latin
letters are measured as. A created document's runs name no East Asian face, so the layout draws their
East Asian text in the fallback face the reference states.

### `modules.json`

Read by `document-engine/tests/docx_layout_modules.rs`, which walks it against the disposition table in `document-engine/src/writing/docx/layout/disposition.rs` in both directions.

| Field | Shape |
| --- | --- |
| `package`, `version` | `"docx"`, `"9.12.0"` |
| `modules` | the 30 `src/…` paths the bundle's region markers name, in bundle order |
| `runtime_markers` | the 7 `\0@oxc-project+runtime@…` helper markers, as the bundle spells them |

### `probes/`

Scripts that report what a created document reaches, each named by the disposition table rows whose
gap it backs; run one as `bun document-engine/assets/docx-layout/probes/<name>.mjs`. They are
documentation: the layout reads none of them, so they are not part of the data's identity.

## Licence

`LICENSE` is the package's own licence, copied verbatim. It covers the parts captured in
`../docx-create/` too: the same library writes them, and `parity/src/bin/capture-docx-parts.rs`
regenerates that directory from the reference's output, so it carries no licence file of its own.

The package's licence does not settle the data's provenance. The package states that its Arabic and
supplementary width tables were generated from the proprietary application's own PDFs, and that its
layout is checked against that application's output, but it does not name the openly licensed fonts
behind the core tables. Part of this data is therefore derived from another vendor's application and
fonts, and the package's licence cannot convey rights its author did not hold. That question is
unresolved, and it is the owner's decision.

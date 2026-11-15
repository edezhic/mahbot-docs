//! The disposition of every module the layout pass reaches, and the module list it is checked against.
//!
//! # The pinned library and its version
//!
//! The reference's layout is the `docx/layout` entry point of the npm package `docx`, at **9.12.0** —
//! the version `assets/docgen/package.json` pins and `bun.lock` resolves. An upgrade of that library
//! is a separate deliberate change: it re-derives the width data ([`super::text`]), the layout
//! constants ([`super::constants`]) and the module list (`assets/docx-layout/modules.json`), because
//! all three are read from the package rather than restated. Nothing about a dependency bump may
//! change them silently.
//!
//! # The table
//!
//! One row per source module the bundle carries, saying what this port does with it. A row is
//! `Ported` when the module's behaviour is transliterated, `Partly` when the behaviour a created
//! document can reach is transliterated and the rest is not, and `Excluded` when a created document
//! cannot reach it at all. A `Ported` row states no note and names no probe, because there is no gap
//! for either to describe; every other row states the behaviour of a created document that cannot
//! reach the module and names the probe the statement rests on. The probe scripts are committed under
//! `document-engine/assets/docx-layout/probes/` and each runs as
//! `bun document-engine/assets/docx-layout/probes/<name>.mjs`, printing one JSON line.
//!
//! The table's coverage is checked in two directions. `assets/docx-layout/modules.json` is the
//! committed module list, derived from the package by
//! `bun parity/scripts/derive-docx-layout-data.mjs`, which refuses to write a list that omits a
//! source module it found. `document-engine/tests/docx_layout_modules.rs` walks that list against
//! this table in the isolated copy, where the package does not exist, and holds every row that is
//! not a full port to a probe the crate ships — and every shipped probe to a row.
//!
//! # What the table does not claim
//!
//! The rows' prose is not machine-checkable, and this module does not pretend it is. What is checked
//! is that every module in the list has a row, every row names a module in the list, a `Ported` row
//! states no note and names no probe, every other row states a note and names a probe the crate
//! ships, and every probe the crate ships is named by a row — so an exclusion cannot be stated
//! without the script behind it, and a script cannot outlive the row that cites it.

/// What this port does with one module of the pinned library's layout.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Disposition {
    /// Transliterated in full.
    Ported,
    /// The behaviour a created document can reach is transliterated; the rest is not.
    Partly,
    /// A created document cannot reach the module at all.
    Excluded,
}

/// One row of the disposition table.
pub struct Row {
    /// The module's own path inside the package, as the bundle's region markers spell it.
    pub module: &'static str,
    pub disposition: Disposition,
    /// The behaviour of a created document that cannot reach the module, and what the probe for it
    /// observed. `None` on a row that is a full port, which has no gap to describe.
    pub note: Option<&'static str>,
    /// The probe script under `assets/docx-layout/probes/` the note rests on. `None` on a row that is
    /// a full port.
    pub probe: Option<&'static str>,
}

/// Every source module the layout bundle carries, and what this port does with it.
pub const TABLE: &[Row] = &[
    Row {
        module: "src/text-layout/arabic-widths.ts",
        disposition: Disposition::Ported,
        note: None,
        probe: None,
    },
    Row {
        module: "src/text-layout/arabic-shaping.ts",
        disposition: Disposition::Ported,
        note: None,
        probe: None,
    },
    Row {
        module: "src/text-layout/font-widths.ts",
        disposition: Disposition::Ported,
        note: None,
        probe: None,
    },
    Row {
        module: "src/text-layout/font-kerning.ts",
        disposition: Disposition::Excluded,
        note: Some(
            "A created document's runs state no `w:kern`, so `measureTextWidthAsDrawn` is `measureTextWidth` and the kerning and ligature tables are never consulted. The probe packs a created document and reports `runsAskingForKerning: 0` and `asDrawnEqualsUnshaped: true` for its text.",
        ),
        probe: Some("kerning.mjs"),
    },
    Row {
        module: "src/text-layout/kerning.ts",
        disposition: Disposition::Excluded,
        note: Some(
            "As font-kerning: no run asks for kerning or ligatures, so the shaping path is never entered and this module's tables are never read. The probe reports that no run carries `w:kern` and that the reference's drawn width equals its unshaped width for the document's text.",
        ),
        probe: Some("kerning.mjs"),
    },
    Row {
        module: "src/text-layout/more-widths.ts",
        disposition: Disposition::Ported,
        note: None,
        probe: None,
    },
    Row {
        module: "src/text-layout/text-width.ts",
        disposition: Disposition::Ported,
        note: None,
        probe: None,
    },
    Row {
        module: "src/text-layout/line-break-rules.ts",
        disposition: Disposition::Partly,
        note: Some(
            "The break opportunities are ported for a document whose runs state no language and whose paragraphs turn word wrapping on: a zero-width space, a dash outside a number, and the Chinese and Japanese characters the compatibility mode breaks between. Word's kinsoku lists — the characters that may not start or end a line — are gated on a run's East Asian language, and the `anywhere` rule on a paragraph that turns word wrapping off; a created document states neither, so neither veto can fire. The probe reports that no run of a created document names a language.",
        ),
        probe: Some("numbering.mjs"),
    },
    Row {
        module: "src/text-layout/text-styles.ts",
        disposition: Disposition::Partly,
        note: Some(
            "The reference's own defaults are what a created document's empty `docDefaults` leaves standing — Times New Roman at ten points, single spacing, a Normal paragraph style with no formatting — and the paragraph and run styles the document states are read. A style naming a theme font is not read: the port resolves no theme font, and a created document states none, nor a `w:rFonts` at all. The probe reports that no run of a created document names a face.",
        ),
        probe: Some("fonts.mjs"),
    },
    Row {
        module: "src/text-layout/line-breaking.ts",
        disposition: Disposition::Ported,
        note: None,
        probe: None,
    },
    Row {
        module: "src/text-layout/office-cloud-fonts.ts",
        disposition: Disposition::Excluded,
        note: Some(
            "A created document names no font of its own: its runs carry no `w:rFonts`, so no run names a cloud font and none is fetched. Its text measures in the default face the width tables state, and its East Asian characters in the fallback East Asian face those tables state. The probe packs a created document and reports that no run states a face or a size and that no font is embedded.",
        ),
        probe: Some("fonts.mjs"),
    },
    Row {
        module: "src/text-layout/font-file.ts",
        disposition: Disposition::Excluded,
        note: Some(
            "A created document embeds no font, so nothing is measured from a font file. The probe reports that the package holds no embedded font.",
        ),
        probe: Some("fonts.mjs"),
    },
    Row {
        module: "src/layout/column-widths.ts",
        disposition: Disposition::Partly,
        note: Some(
            "The fitting is ported for a table whose cells give no widths of their own, which is the table a created document writes: each column as wide as the widest line of its cells, then widened or narrowed to the width the table gives itself. What a created table cannot reach is not ported: cells that state their own widths, cells merged across columns, text that runs up or down a cell, a table laid out fixed, and space between cells. The probe reports that the row of a table whose cells hold unequal text is one line tall and the row of the same text beside itself two, which is what sizing the columns to their content gives and the grid's equal columns do not.",
        ),
        probe: Some("table.mjs"),
    },
    Row {
        module: "src/layout/floating-drawings.ts",
        disposition: Disposition::Excluded,
        note: Some(
            "A created document's pictures are inline drawings, which the line they stand in carries; no text flows around one. The probe packs a document with a sized picture and reports that it is an inline drawing and no floating one was placed.",
        ),
        probe: Some("floating.mjs"),
    },
    Row {
        module: "src/layout/number-format.ts",
        disposition: Disposition::Partly,
        note: Some(
            "The two formats a created document's numbering states are written: a decimal list's marker is its number, and a bullet's is its character. The other formats Word writes list numbers in are not, and a level stating one has its `%1` replaced by nothing, as the reference's substitution of a number it cannot format gives. The reader also takes only a definition's level 0, which is the one level a created document's list items stand at. The probe reports that the numbering part of a created document states `bullet` and `decimal` only.",
        ),
        probe: Some("numbering.mjs"),
    },
    Row {
        module: "src/layout/bound-controls.ts",
        disposition: Disposition::Excluded,
        note: Some(
            "A content control bound to custom XML is filled in by Word when it opens a document, and a created document binds none: the one `w:sdt` its body carries is the table of contents, which states no `w:dataBinding`, and the package holds no custom XML part for one to name. The probe packs a document that names a table of contents and reports its body's elements as `[\"w:sdt\"]`, no `w:dataBinding` among them and no custom XML part.",
        ),
        probe: Some("parts.mjs"),
    },
    Row {
        module: "src/layout/cambria-math.ts",
        disposition: Disposition::Excluded,
        note: Some(
            "A created document carries no equation, and the only mathematical character a case places is one whose width the measuring face does not state, which stops the layout rather than measuring it. The probe packs a document holding U+2207 and reports that its page count stays blank where the summation and integral signs settle.",
        ),
        probe: Some("nabla.mjs"),
    },
    Row {
        module: "src/layout/equations.ts",
        disposition: Disposition::Excluded,
        note: Some(
            "As cambria-math: a created document's blocks carry no `m:oMath`. The probe packs a document and reports that its body's elements are `[\"w:sdt\"]`, which holds no equation.",
        ),
        probe: Some("parts.mjs"),
    },
    Row {
        module: "src/layout/table-formats.ts",
        disposition: Disposition::Partly,
        note: Some(
            "The room a table's borders take is read from the parts: the left and right ones off the columns the table gives itself, and the top, inside and bottom ones around each of its rows. That is the only formatting a created table states beyond its margins — single-line borders of half a point and a width as a share of the text column. What a created table cannot state is not read: its cells' own borders, the space between its cells, a table style's conditional formatting, and the rooms of double, triple, art, 3D and wave borders. The probe reports where the layout stops a table given no widths — at forty-two columns of one character, where a cell's margins are wider than the room its column gets — and that a table whose cells hold unequal text is one line tall.",
        ),
        probe: Some("table.mjs"),
    },
    Row {
        module: "src/layout/text-frames.ts",
        disposition: Disposition::Excluded,
        note: Some(
            "A created document's paragraphs carry no frame, and its pictures are inline. The probe reports that no paragraph carries a frame.",
        ),
        probe: Some("floating.mjs"),
    },
    Row {
        module: "src/layout/vml-drawings.ts",
        disposition: Disposition::Excluded,
        note: Some(
            "A created document writes no VML: its pictures are DrawingML inline drawings. The probe packs a document with a picture and reports that it holds no `w:pict` element.",
        ),
        probe: Some("vml.mjs"),
    },
    Row {
        module: "src/layout/read-document.ts",
        disposition: Disposition::Partly,
        note: Some(
            "The reader is ported for the shapes a created document carries — paragraphs, runs, fields, tables, inline pictures, lists, footnotes, one section, a header and a footer. A shape a created document does not write is not read at all: a text box, an equation, a VML drawing, a floating drawing and an imported document are skipped rather than carried as markers, so the pass lays out the shapes it knows and nothing else. A `w:sdt` is descended into rather than skipped, which is how the table of contents' entries are read. The probe reports the part names a created package holds, that its body carries a `w:sdt` and no text box or equation, and that the control binds nothing.",
        ),
        probe: Some("parts.mjs"),
    },
    Row {
        module: "src/layout/paginate.ts",
        disposition: Disposition::Partly,
        note: Some(
            "The layout is ported for a document of one section, one column, inline pictures, tables whose rows break across pages, footnotes at the page's bottom and a header and a footer. What a created document cannot reach is not ported: more than one section, more than one column, balanced columns before a continuous section break, text flowing around a drawing, a text box or frame, an equation, a footnote that continues past the page it is referred to on, widow and orphan control and a table row that may not split — the created styles state no `w:widowControl` and the created tables no `w:cantSplit` — and every compatibility mode older than the one a created document states. The probe reports one section, no column element and compatibility mode 15.",
        ),
        probe: Some("sections.mjs"),
    },
    Row {
        module: "src/layout/layout-passes.ts",
        disposition: Disposition::Ported,
        note: None,
        probe: None,
    },
    Row {
        module: "src/layout/measure-width.ts",
        disposition: Disposition::Excluded,
        note: Some(
            "That module measures through a canvas — an `OffscreenCanvas` or a page's own — and is a measurer a caller may hand the library rather than the one it uses; the kit never hands one, so a created document's pages are the width tables' own. The probe reports that this environment has no canvas at all, so the module cannot run here whatever a caller asks for.",
        ),
        probe: Some("fonts.mjs"),
    },
    Row {
        module: "src/layout/image-sizes.ts",
        disposition: Disposition::Partly,
        note: Some(
            "The layout takes a picture's size from the extent its drawing states, which is what a created document writes. Working a size out from an image file's own pixels and resolution is not ported, and no created drawing reaches it. The probe reports that the document's one drawing is inline and states its own extent.",
        ),
        probe: Some("floating.mjs"),
    },
    Row {
        module: "src/layout/imported-documents.ts",
        disposition: Disposition::Excluded,
        note: Some(
            "A created document imports nothing: its body's elements carry no `w:altChunk`, and its package holds no custom XML part. The probe packs a document and reports the body's elements as `[\"w:sdt\"]` and the package's parts as a created document's own.",
        ),
        probe: Some("parts.mjs"),
    },
    Row {
        module: "src/layout/read-docx.ts",
        disposition: Disposition::Ported,
        note: None,
        probe: None,
    },
    Row {
        module: "src/layout/estimate-page-numbers.ts",
        disposition: Disposition::Ported,
        note: None,
        probe: None,
    },
    Row {
        module: "src/layout/layout-document.ts",
        disposition: Disposition::Excluded,
        note: Some(
            "That module converts the laid-out pages to pixels for a caller that wants the layout itself; a created document wants only the numbers, which the passes answer before any conversion. The probe hands the reference's own entry point the formatted body and the context the packer hands it, and reports the answer: the page count, the bookmarks, their page numbers, the sections' counts and the relative positions, with no pixel geometry among them — one page for one paragraph and four for two hundred, so the answer is one the passes worked out and not a not-settled default.",
        ),
        probe: Some("pages.mjs"),
    },
];

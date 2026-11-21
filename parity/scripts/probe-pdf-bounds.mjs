#!/usr/bin/env bun
// Measures the reference on the shapes the engine's PDF bounds are held against, and prints the table
// `document-engine/VOCABULARY.md` records beside each bound.
//
// Those bounds exist for the engine's own cost, not for the file, so what justifies one is what the
// reference does with the same shape: a shape it reads in a fraction of a second is not one the engine
// may refuse, and a shape it gives up on is one a bound may stop at. Every probe drives the committed
// kit through the request-and-result contract the harness uses — the same bundle, the same request
// shape, the same result file — and nothing here is part of a run.
//
//   bun parity/scripts/probe-pdf-bounds.mjs
//
// Scratch files go under the OS temporary directory and are removed.

import { mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { deflateSync } from "node:zlib";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { spawnSync } from "node:child_process";

const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "../..");
const KIT = join(ROOT, "assets/docgen/document-kit.js");
const FONT = join(ROOT, "assets/docgen/NotoSans-Regular.ttf");
const BUN = process.env.PARITY_BUN || "bun";
const A4 = "[0 0 595.28 841.89]";
const CONTENT = (text, at) => `BT /F1 24 Tf 72 ${at} Td (${text}) Tj ET`;

// A file of `objects` — bodies numbered from one, in the order given — and a classic index. A body
// that is a `Buffer` is written as its own bytes, which is how a stream is stated.
const file = (objects, root) => {
  const pieces = [Buffer.from("%PDF-1.7\n%\xe2\xe3\xcf\xd3\n", "latin1")];
  const offsets = [];
  let length = pieces[0].length;
  objects.forEach((body, index) => {
    offsets.push(length);
    const piece = Buffer.concat([
      Buffer.from(`${index + 1} 0 obj\n`, "latin1"),
      Buffer.isBuffer(body) ? body : Buffer.from(body, "latin1"),
      Buffer.from("\nendobj\n", "latin1"),
    ]);
    pieces.push(piece);
    length += piece.length;
  });
  const entries = offsets
    .map((offset) => `${String(offset).padStart(10, "0")} 00000 n \n`)
    .join("");
  pieces.push(
    Buffer.from(
      `xref\n0 1\n0000000000 65535 f \n1 ${offsets.length}\n${entries}` +
        `trailer\n<< /Size ${offsets.length + 1} /Root ${root} >>\nstartxref\n${length}\n%%EOF\n`,
      "latin1",
    ),
  );
  return Buffer.concat(pieces);
};

const stream = (dictionary, data) =>
  Buffer.concat([
    Buffer.from(`<< /Length ${data.length} ${dictionary} >>\nstream\n`, "latin1"),
    data,
    Buffer.from("\nendstream", "latin1"),
  ]);

// One A4 page drawing one line, a font, a page tree and a catalog: the file every probe is a
// variation of.
const page = (extra = [], contents = "3 0 R") =>
  file(
    [
      "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>",
      "<< /Type /Pages /Kids [4 0 R] /Count 1 >>",
      stream("", Buffer.from(CONTENT("Page one text", 760), "latin1")),
      `<< /Type /Page /Parent 2 0 R /MediaBox ${A4} /Resources << /Font << /F1 1 0 R >> >> /Contents ${contents} >>`,
      "<< /Type /Catalog /Pages 2 0 R >>",
      ...extra,
    ],
    "5 0 R",
  );

const nested = (depth) => "[".repeat(depth) + "]".repeat(depth);

// One A4 page whose own dictionary holds a value nested `depth` deep: the shape `object_depth_max` is
// measured against, because the page must still be found and turned when the value parses.
const pageWith = (depth, contents = "3 0 R") =>
  file(
    [
      "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>",
      "<< /Type /Pages /Kids [4 0 R] /Count 1 >>",
      stream("", Buffer.from(CONTENT("Page one text", 760), "latin1")),
      `<< /Type /Page /Parent 2 0 R /MediaBox ${A4} /Resources << /Font << /F1 1 0 R >> >> /Contents ${contents} /Deep ${nested(depth)} >>`,
      "<< /Type /Catalog /Pages 2 0 R >>",
    ],
    "5 0 R",
  );

// A page-less file whose page tree states `count` as its `/Count`: the reference faults on it, and the
// engine produces, which is a divergence the crate states rather than a bound.
const pageLess = (count) =>
  file(
    [
      "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>",
      `<< /Type /Pages /Kids [] /Count ${count} >>`,
      "<< /Type /Catalog /Pages 2 0 R >>",
    ],
    "3 0 R",
  );

// A page tree that names itself: `traverse` recurses until the runtime throws, and the load turns that
// into the refusal both sides print.
const pageTreeNamesItself = () =>
  file(
    [
      "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>",
      "<< /Type /Pages /Kids [2 0 R] /Count 1 >>",
      stream("", Buffer.from(CONTENT("Page one text", 760), "latin1")),
      "<< /Type /Catalog /Pages 2 0 R >>",
    ],
    "4 0 R",
  );

// The same page whose `/Parent` returns to where it began: the reference's inherited-entry walk never
// stops, so its load dies with `Maximum call stack size exceeded` and the answer is a fault.
const parentCycle = () =>
  file(
    [
      "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>",
      "<< /Type /Pages /Kids [4 0 R] /Count 1 /Parent 4 0 R >>",
      stream("", Buffer.from(CONTENT("Page one text", 760), "latin1")),
      `<< /Type /Page /Parent 2 0 R /MediaBox ${A4} /Resources << /Font << /F1 1 0 R >> >> /Contents 3 0 R >>`,
      "<< /Type /Catalog /Pages 2 0 R >>",
    ],
    "5 0 R",
  );

// A `/Root` whose `/Type` is a reference, with a proper catalog in the body: the reference builds a
// catalog only for a dictionary that says it is one, so it passes the entry over and adopts the body's
// own catalog — measured, it produces, and so does the engine.
const rootTypeByReference = () =>
  file(
    [
      "<< /Type /Catalog /Pages 2 0 R >>",
      "<< /Type /Pages /Kids [4 0 R] /Count 1 >>",
      "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>",
      `<< /Type /Page /Parent 2 0 R /MediaBox ${A4} /Resources << /Font << /F1 3 0 R >> >> /Contents 7 0 R >>`,
      "<< /Type 1 0 R /Pages 2 0 R >>",
      "<< /Type /Catalog /Pages 2 0 R >>",
      stream("", Buffer.from(CONTENT("Page one text", 760), "latin1")),
    ],
    "5 0 R",
  );

// A catalog that states no `/Type`: the reference builds a catalog only for a dictionary that says it
// is one, so its load refuses the file, and so does the engine.
const catalogWithoutType = () =>
  file(
    [
      "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>",
      "<< /Type /Pages /Kids [4 0 R] /Count 1 >>",
      stream("", Buffer.from(CONTENT("Page one text", 760), "latin1")),
      `<< /Type /Page /Parent 2 0 R /MediaBox ${A4} /Resources << /Font << /F1 1 0 R >> >> /Contents 3 0 R >>`,
      "<< /Pages 2 0 R >>",
    ],
    "5 0 R",
  );

// A page tree whose every level names its child twice: the walk visits it twice per level, so `depth`
// levels is 2^depth visits of one page.
const doublingPageTree = (depth) => {
  const objects = [
    "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>",
    "<< /Type /Pages /Kids [] /Count 1 >>",
    stream("", Buffer.from(CONTENT("Page one text", 760), "latin1")),
    `<< /Type /Page /Parent 2 0 R /MediaBox ${A4} /Resources << /Font << /F1 1 0 R >> >> /Contents 3 0 R >>`,
    "<< /Type /Catalog /Pages 2 0 R >>",
  ];
  let current = 4;
  for (let index = 0; index < depth; index += 1) {
    objects.push(`<< /Type /Pages /Kids [${current} 0 R ${current} 0 R] /Count 1 >>`);
    current = objects.length;
  }
  objects[1] = `<< /Type /Pages /Kids [${current} 0 R] /Count 1 >>`;
  return file(objects, "5 0 R");
};

// An object stream whose every entry states the same offset: the font the page draws with is held
// thirty-two times, each entry naming the same bytes.
const oneOffsetContainer = () => {
  const header = "6 0 ".repeat(32);
  const data = Buffer.concat([
    Buffer.from(header, "latin1"),
    Buffer.from("<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>", "latin1"),
  ]);
  return file(
    [
      "<< /Type /Pages /Kids [2 0 R] /Count 1 >>",
      `<< /Type /Page /Parent 1 0 R /MediaBox ${A4} /Resources << /Font << /F1 6 0 R >> >> /Contents 3 0 R >>`,
      stream("", Buffer.from(CONTENT("Page one text", 760), "latin1")),
      stream(`/Type /ObjStm /N 32 /First ${header.length}`, data),
      "<< /Type /Catalog /Pages 1 0 R >>",
    ],
    "5 0 R",
  );
};

// An object stream whose two entries state `numbers`, each naming a dictionary of its own: the
// header states every number beside the offset of its body in the container's data.
const containerNaming = (numbers) => {
  const bodies = [
    "<< /Type /Annot /Rect [0 0 1 1] /NM (in-container-one) >>",
    "<< /Type /Annot /Rect [1 1 2 2] /NM (in-container-two) >>",
  ];
  let header = "";
  let body = "";
  numbers.forEach((number, index) => {
    header += `${number} ${body.length} `;
    body += bodies[index];
  });
  const data = Buffer.from(header + body, "latin1");
  return page([stream(`/Type /ObjStm /N 2 /First ${header.length}`, data)]);
};

// A page tree whose `/Kids` is a reference that names a reference: the reference's typed lookup takes
// one step and finds a reference rather than the array it needs.
const kidsByReference = () =>
  file(
    [
      "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>",
      "<< /Type /Pages /Kids 6 0 R /Count 1 >>",
      `<< /Type /Page /Parent 2 0 R /MediaBox ${A4} /Resources << /Font << /F1 1 0 R >> >> /Contents 4 0 R >>`,
      stream("", Buffer.from(CONTENT("Page one text", 760), "latin1")),
      "[3 0 R]",
      "5 0 R",
      "<< /Type /Catalog /Pages 2 0 R >>",
    ],
    "7 0 R",
  );

// The same for a page's `/MediaBox`: its lookup is not a `lookupMaybe`, so a reference that names a
// reference is not four numbers and the resize faults.
const mediaBoxByReference = () =>
  file(
    [
      "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>",
      "<< /Type /Pages /Kids [3 0 R] /Count 1 >>",
      `<< /Type /Page /Parent 2 0 R /MediaBox 6 0 R /Resources << /Font << /F1 1 0 R >> >> /Contents 4 0 R >>`,
      stream("", Buffer.from(CONTENT("Page one text", 760), "latin1")),
      A4,
      "5 0 R",
      "<< /Type /Catalog /Pages 2 0 R >>",
    ],
    "7 0 R",
  );

// One page beside `count` objects numbered past the model and a reference naming the format's object
// ceiling: the shape the search for a free number is measured on.
const manyPastModel = (count) => {
  const pieces = [
    page(),
    Buffer.from("\n200 0 obj\n<< /Type /Annot /Rect 4294967295 0 R >>\nendobj\n", "latin1"),
  ];
  for (let index = 0; index < count; index += 1) {
    pieces.push(Buffer.from(`\n${5_000_000_000 + index} 0 obj\nnull\nendobj\n`, "latin1"));
  }
  return Buffer.concat(pieces);
};

// A page beside `count` stream objects that state a length and never close: each states a `/Length`
// far beyond the bytes it carries and no `endstream` follows any of them, so a reader that looks for
// one spends the look and steps on.
const damagedStreams = (count) => {
  const pieces = [page()];
  const data = "x".repeat(40);
  for (let index = 0; index < count; index += 1) {
    pieces.push(
      Buffer.from(
        `\n${6 + index} 0 obj\n<< /Length 99999 >> stream\n${data}\nendobj\n`,
        "latin1",
      ),
    );
  }
  return Buffer.concat(pieces);
};

// One A4 page whose `/CropBox` is null beside the `/MediaBox` it states: the entry the crop reads.
const cropBoxNull = () =>
  file(
    [
      "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>",
      "<< /Type /Pages /Kids [4 0 R] /Count 1 >>",
      stream("", Buffer.from(CONTENT("Page one text", 760), "latin1")),
      `<< /Type /Page /Parent 2 0 R /MediaBox ${A4} /CropBox null /Resources << /Font << /F1 1 0 R >> >> /Contents 3 0 R >>`,
      "<< /Type /Catalog /Pages 2 0 R >>",
    ],
    "5 0 R",
  );

// A page whose `/Resources` is a reference that names a reference: the reference's typed lookup takes
// one step and finds a reference rather than the dictionary it needs.
const resourcesByReference = () =>
  file(
    [
      "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>",
      "<< /Type /Pages /Kids [4 0 R] /Count 1 >>",
      stream("", Buffer.from(CONTENT("Page one text", 760), "latin1")),
      `<< /Type /Page /Parent 2 0 R /MediaBox ${A4} /Resources 6 0 R /Contents 3 0 R >>`,
      "<< /Type /Catalog /Pages 2 0 R >>",
      "7 0 R",
      "<< /Font << /F1 1 0 R >> >>",
    ],
    "5 0 R",
  );

// A catalog whose `/AcroForm` is a reference that names a reference: the fill's typed lookup takes one
// step and finds a reference rather than the form it needs.
const acroFormByReference = () =>
  file(
    [
      "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>",
      "<< /Type /Pages /Kids [4 0 R] /Count 1 >>",
      stream("", Buffer.from(CONTENT("Page one text", 760), "latin1")),
      `<< /Type /Page /Parent 2 0 R /MediaBox ${A4} /Resources << /Font << /F1 1 0 R >> >> /Contents 3 0 R /Annots [8 0 R] >>`,
      "<< /Type /Catalog /Pages 2 0 R /AcroForm 6 0 R >>",
      "7 0 R",
      "<< /Fields [8 0 R] /DA (/Helv 0 Tf 0 g) /DR << /Font << /Helv 1 0 R >> >> >>",
      "<< /Type /Annot /Subtype /Widget /FT /Tx /T (name) /Rect [72 700 272 724] /F 4 /P 4 0 R /DA (/Helv 0 Tf 0 g) >>",
    ],
    "5 0 R",
  );

// A page beside two objects numbered past what a `u64` holds, stated in two runs of digits that are
// two doubles: the reference reads a run with `Number`, so it keys two objects and keeps both bodies.
const numbersKeyedApart = () =>
  Buffer.concat([
    page(),
    Buffer.from(
      "\n18446744073709551616 0 obj\n<< /Type /Annot /Rect [0 0 1 1] /NM (kept-apart-one) >>\nendobj\n" +
        "36893488147419103232 0 obj\n<< /Type /Annot /Rect [1 1 2 2] /NM (kept-apart-two) >>\nendobj\n",
      "latin1",
    ),
  ]);

// One container whose two entries state those two numbers, each naming a dictionary of its own: the
// reference keys each entry's number by the double its digits state, so it keeps both.
const containerNamingNumbersKeyedApart = () =>
  containerNaming(["18446744073709551616", "36893488147419103232"]);

// The same container with both entries stating one number in two runs: both round to one double, so
// they are one number to the reference and one of the two bodies is dropped.
const containerNamingOneNumberInTwoRuns = () =>
  containerNaming(["18446744073709551616", "18446744073709551617"]);

// One number stated twice in runs that round to one double, the second as a body that does not
// parse: the reference keeps one object there, and the second body's bytes are what it holds.
const carriedBodyReplacingANumber = () =>
  Buffer.concat([
    page(),
    Buffer.from(
      "\n18446744073709551616 0 obj\n(NEEDLE)\nendobj\n" +
        "18446744073709551617 0 obj\n<< broken\nendobj\n",
      "latin1",
    ),
  ]);

// A page carrying a form whose `/Fields` entry is `[${fields}]` — or `entry`, for a form that states the
// list through a reference to it — and whose page annotations are `annots`, with `extra` objects
// appended after it (those numbered from seven).
const formPage = (fields, annots, extra, entry = `[${fields}]`) =>
  file(
    [
      "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>",
      "<< /Type /Pages /Kids [4 0 R] /Count 1 >>",
      stream("", Buffer.from(CONTENT("Page one text", 760), "latin1")),
      `<< /Type /Page /Parent 2 0 R /MediaBox ${A4} /Resources << /Font << /F1 1 0 R >> >> /Contents 3 0 R /Annots [${annots}] >>`,
      "<< /Type /Catalog /Pages 2 0 R /AcroForm 6 0 R >>",
      `<< /Fields ${entry} /DA (/Helv 0 Tf 0 g) /DR << /Font << /Helv 1 0 R >> >> >>`,
      ...extra,
    ],
    "5 0 R",
  );

// One widget of the shape a probe's form states, over the form page's box.
const widget = (states) =>
  `<< /Type /Annot /Subtype /Widget /Rect [72 700 272 724] /F 4 /P 4 0 R /DA (/Helv 0 Tf 0 g) ${states} >>`;

// A text field whose `/Parent` is a reference that names a reference: the fill reads a field's
// inherited entries by ascending that chain with a typed lookup, which takes one step and finds a
// reference rather than the dictionary it needs.
const fieldParentByReference = () =>
  formPage("8 0 R", "8 0 R", [
    "<< /T (name) >>",
    widget("/FT /Tx /T (name) /Parent 9 0 R"),
    "7 0 R",
  ]);

// The same form whose `/Fields` entry is a reference that names a reference: the field tree is built
// eagerly with the same one-step lookup.
const fieldsByReference = () => formPage("8 0 R", "7 0 R", [widget("/FT /Tx /T (name)"), "7 0 R"]);

// A form whose `/Fields` entry names an array of its own: `PDFAcroForm.Fields` takes one step, so the
// list is read and the field it holds filled.
const fieldsIndirectly = () =>
  formPage("", "8 0 R", ["[8 0 R]", widget("/FT /Tx /T (name)")], "7 0 R");

// A container whose `/Kids` names an array of its own, holding one named child: `isNonTerminalAcroField`
// and `PDFAcroField.Kids` both take one step, so the child is a field of the reference's.
const kidsIndirectly = () =>
  formPage("7 0 R", "8 0 R", [
    "<< /T (P) /Kids 9 0 R >>",
    widget("/FT /Tx /T (Q) /Parent 7 0 R"),
    "[8 0 R]",
  ]);

// A container whose `/Kids` states a field dictionary in place: `createPDFAcroFields` keeps a kid only
// when the array states a reference, so the reference has no field at all.
const kidInPlace = () =>
  formPage("7 0 R", "", [
    `<< /T (P) /Kids [${widget("/FT /Tx /T (Q) /Parent 7 0 R")}] >>`,
  ]);

// A text field stating `/FT null` under a parent that states `/FT /Tx`: `getInheritableAttribute` skips
// a stated null, so the parent's type is the field's.
const nullTypeInherited = () =>
  formPage("7 0 R", "8 0 R", [
    "<< /T (P) /FT /Tx /Kids [8 0 R] >>",
    widget("/FT null /T (Q) /Parent 7 0 R"),
  ]);

// A text field whose own `/T` is a reference that names nothing: `lookupMaybe` answers undefined for
// one, as for an absent entry, so the field is named by its chain's other links.
const nameNamesNothing = () =>
  formPage("7 0 R", "8 0 R", [
    "<< /T (P) /Kids [8 0 R] >>",
    widget("/FT /Tx /T 99 0 R /Parent 7 0 R"),
  ]);

// Two text fields, the first filled and the second not: the second states a normal appearance and a
// `/Ff` that is a reference to a reference, which the reference reads only where it regenerates that
// field's appearance.
const cleanFieldWhoseFlagsAreAReference = () =>
  formPage("7 0 R 8 0 R", "7 0 R 8 0 R", [
    widget("/FT /Tx /T (f0)"),
    widget("/FT /Tx /T (f1) /Ff 10 0 R /AP << /N 9 0 R >>"),
    stream("", Buffer.from("", "latin1")),
    "11 0 R",
    "0",
  ]);

// The same field as the one above, filled by the request: the appearance the fill regenerates reads the
// flags, and the typed read throws.
const filledFieldWhoseFlagsAreAReference = () =>
  formPage("7 0 R", "7 0 R", [widget("/FT /Tx /T (f0) /Ff 8 0 R"), "9 0 R", "0"]);

// A choice field whose `/Ff` is a reference that names nothing: `Ff()` is a `lookupMaybe`, which answers
// undefined for a value that names nothing, so the flags are zero and the field is a list box.
const choiceWhoseFlagsNameNothing = () =>
  formPage("7 0 R", "7 0 R", [widget("/FT /Ch /T (f0) /Ff 99 0 R /Opt [(a) (b)]")]);

// A choice field whose `/Ff` is the negative real `-1.5`: `hasFlag` tests `getFlags() & flag`, and
// JavaScript's `&` converts `-1.5` to the 32-bit integer `-1`, so every bit is set and the field is a
// combo box that takes a value outside its own list.
const choiceFlagsNegativeReal = () =>
  formPage("7 0 R", "7 0 R", [widget("/FT /Ch /T (f0) /Ff -1.5 /Opt [(a) (b)]")]);

// The same real on a button: the 32-bit read sets the push-button bit, which `createPDFAcroButton`
// tests before the radio bit, so the field is one no fill can take a value for.
const buttonFlagsNegativeReal = () =>
  formPage("7 0 R", "7 0 R", [widget("/FT /Btn /T (f0) /Ff -1.5")]);

// A choice field whose `/Ff` is a real past 2^32: the low 32 bits hold no bit the kind reads, so the
// field is a list box and the value the request gives is not one of its own.
const choiceFlagsHugeReal = () =>
  formPage("7 0 R", "7 0 R", [
    widget("/FT /Ch /T (f0) /Ff 100000000000000000000.0 /Opt [(a) (b)]"),
  ]);

// A real the reference reads as a double and this model as a 32-bit float: `4294967295.5` is `-1`
// there (a combo box, produced) and 2^32 here (a list box, refused). The band is stated in
// `PARSER.md` §5.16.
const choiceFlagsPastTheFloat = () =>
  formPage("7 0 R", "7 0 R", [widget("/FT /Ch /T (f0) /Ff 4294967295.5 /Opt [(a) (b)]")]);

// A whole number past 2^53: the reference reads the digits as a double, and 2^53 + 131071 rounds up
// onto the combo bit that the exact integer's low 32 bits do not hold.
const choiceFlagsPastTheExactInteger = () =>
  formPage("7 0 R", "7 0 R", [widget("/FT /Ch /T (f0) /Ff 9007199254872063 /Opt [(a) (b)]")]);

// A form of two containers whose `/Kids` name each other: `getFields` recurses on a non-terminal field
// and dies with a stack overflow, where the engine's walk ends at the cycle (`PARSER.md` §5.17).
const fieldKidsCycle = () =>
  formPage("7 0 R", "", ["<< /T (P) /Kids [8 0 R] >>", "<< /T (Q) /Kids [7 0 R] >>"]);

// A text field whose `/Parent` names itself: `getFullyQualifiedName` and the constructor's `ascend`
// both recurse on the link and die with a stack overflow.
const fieldParentCycle = () =>
  formPage("7 0 R", "7 0 R", [widget("/FT /Tx /T (P) /Parent 7 0 R")]);

// A text field whose `/MaxLen` is a real: `MaxLen()` is `lookupMaybe(…, PDFNumber)`, so the reference
// compares the text's length against the real and faults with `maxLength=3` for 3.0 and
// `maxLength=3.5` for the fraction. The engine reads only a whole number (`PARSER.md` §5.16).
const textMaxLenWholeReal = () =>
  formPage("7 0 R", "7 0 R", [widget("/FT /Tx /T (f0) /MaxLen 3.0")]);

const textMaxLenFraction = () =>
  formPage("7 0 R", "7 0 R", [widget("/FT /Tx /T (f0) /MaxLen 3.5")]);

// A text field whose `/Parent` is a dictionary stated in place: the constructor's `ascend` reads each link
// with `lookupMaybe(…, PDFDict)`, which follows a dictionary stated in place, so the field it inherits
// from is the parent's. The name is still its own — `getParent` follows a reference only.
const parentInPlace = () =>
  formPage("7 0 R", "7 0 R", [widget("/FT /Tx /T (Q) /Parent << /FT /Tx /T (P) >>")]);

// A form whose one field's widget kid is not a dictionary, beside a request that names no field: the
// reference applies the values before it reads any widget list, so the answer is the usage refusal.
const missingFieldBesideABadKid = () =>
  formPage("7 0 R", "", [widget("/FT /Tx /T (f0) /Kids [42]")]);

// A text field whose `/Kids` is an empty array: `getWidgets` answers no widget, so the fill writes the
// value and no appearance at all.
const emptyKids = () => formPage("7 0 R", "", [widget("/FT /Tx /T (f0) /Kids []")]);

// A text field whose `/Kids` states a number: the typed widget read throws on it.
const kidIsANumber = () => formPage("7 0 R", "", [widget("/FT /Tx /T (f0) /Kids [42]")]);

// A text field whose `/Kids` is a number rather than an array: the container test reads the entry
// untyped and sees no kids, and `Kids()` throws where the appearance is regenerated.
const kidsIsANumber = () => formPage("7 0 R", "", [widget("/FT /Tx /T (f0) /Kids 42")]);

// One container holding the object `30 0 obj null` at byte `at` of its data, whose entry table names
// that byte as `offset`: the reference places an entry at `/First` plus that offset, so the sign of
// `/First` decides nothing on its own.
const containerLocating = (first, offset, at) => {
  const table = `30 ${offset}\n`;
  const data = table + " ".repeat(at - table.length) + "null";
  return page([
    stream(`/Type /ObjStm /N 1 /First ${first}`, Buffer.from(data, "latin1")),
  ]);
};

// A named non-terminal `P` holding a child named `Q` and one that states no `/T` of its own: the
// reference names them `P.Q` and `P.undefined`, because the name is the chain the `/Parent` entry
// states rather than the tree the form reaches through `/Kids`.
const fieldsNamedThroughTheirChain = () =>
  formPage("7 0 R", "8 0 R 9 0 R", [
    "<< /T (P) /Kids [8 0 R 9 0 R] >>",
    widget("/FT /Tx /T (Q) /Parent 7 0 R"),
    widget("/FT /Tx /Parent 7 0 R"),
  ]);

// Two text fields, the second naming itself with a number: a name the reference reads only where it
// reaches that field, so a fill that matches the first never throws on it.
const fieldNameThatIsNotAString = () =>
  formPage("7 0 R 8 0 R", "7 0 R 8 0 R", [
    widget("/FT /Tx /T (f0)"),
    widget("/FT /Tx /T 42"),
  ]);

// One signature field whose `/Ff` is a reference to a reference: the reference reads no flags for a
// signature, so the form is well formed and the field is one no value can be given.
const signatureWhoseFlagsAreAReference = () =>
  formPage("7 0 R", "7 0 R", [widget("/FT /Sig /T (f0) /Ff 8 0 R"), "9 0 R", "0"]);

// A page beside `count` streams that never close, with one `endstream` at the end of the file: every
// recovery has something to find, and finds it past every stream, so a scan that reads forward from
// each one reads the rest of the file once per stream.
const streamsBeforeOneEnd = (count) => {
  const pieces = [page()];
  const data = "x".repeat(40);
  for (let index = 0; index < count; index += 1) {
    pieces.push(
      Buffer.from(
        `\n${6 + index} 0 obj\n<< /Length 99999 >> stream\n${data}\nendobj\n`,
        "latin1",
      ),
    );
  }
  pieces.push(Buffer.from("\nendstream\n", "latin1"));
  return Buffer.concat(pieces);
};

const probes = {
  // `object_depth_max`: the reference's parser stops where its own stack runs out, and from there it
  // keeps the object as the bytes it states rather than refusing the file. The value sits in a page
  // dictionary, so a run that parses is a page turned and one that does not is a page lost. The point
  // between the two moves with the runtime's own stack rather than being a number: 27 000 parsed in
  // one run and threw in another, so what a row says here is one run's answer.
  "a page holding a value 20 000 deep: parsed, the page turned": () => pageWith(20_000),
  "a page holding a value 27 000 deep: parsed or thrown, run by run": () => pageWith(27_000),
  "a page holding a value 30 000 deep: the object kept as bytes": () => pageWith(30_000),
  // An object no page reaches, for the same bound: carrying it costs nothing either side.
  "an object 30 000 deep that no page reaches": () => page([nested(30_000)]),
  // The one shape this engine produces where the reference faults: a page tree that states no page and
  // a `/Count` that is not a number. Stated as a divergence in `PARSER.md` §5.8, not bounded away.
  "a page tree whose /Count is not a number": () => pageLess(nested(20_000)),
  // A `/Parent` cycle: the reference's inherited-entry walk never returns, so its load faults where the
  // engine produces. Stated in `PARSER.md` §5.9 as a permissive divergence.
  "a page whose /Parent names its own tree": parentCycle,
  // Both sides refuse these two: the shapes are recorded as declined corpus cases.
  "a page tree that names itself": pageTreeNamesItself,
  "a catalog with no /Type": catalogWithoutType,
  // A `/Root` whose `/Type` is a reference: the reference adopts the body's own catalog and produces.
  "a /Root whose /Type is a reference": rootTypeByReference,
  // `container_decode_max_bytes`: a chain of expanding filters multiplies, and the reference stops
  // only where a typed array cannot hold the result — it keeps the container as the bytes it states.
  // The payload `\x81\x81` is a RunLength run of 128 bytes, so each stage is 64 times the one before.
  "a filter chain decoding to 2.1 GiB": () =>
    page([
      stream(
        "/Type /ObjStm /N 0 /First 0 /Filter [ /RunLengthDecode /RunLengthDecode /RunLengthDecode /RunLengthDecode /RunLengthDecode ]",
        Buffer.from([0x81, 0x81]),
      ),
    ]),
  "a filter chain past what a typed array holds": () =>
    page([
      stream(
        "/Type /ObjStm /N 0 /First 0 /Filter [ /RunLengthDecode /RunLengthDecode /RunLengthDecode /RunLengthDecode /RunLengthDecode /RunLengthDecode ]",
        Buffer.from([0x81, 0x81]),
      ),
    ]),
  // `container_decode_max_bytes`: one stage of eighty mebibytes is decoded and produced.
  "container decoding to 80 MiB": () => {
    const payload = Buffer.from("200 0 " + " ".repeat(80 * 1024 * 1024), "latin1");
    const body = deflateSync(payload);
    return page([stream("/Type /ObjStm /N 1 /First 6 /Filter /FlateDecode", body)]);
  },
  // An object numbered 999 999 999: the reference reads such a file and writes one back.
  "an object numbered 999 999 999: the reference produces": () =>
    Buffer.concat([page(), Buffer.from("\n999999999 0 obj\nnull\nendobj\n", "latin1")]),
  // A page tree 5 000 nodes deep: the reference walks it and produces.
  "a page tree 5 000 nodes deep: the reference produces": () => {
    const objects = [
      "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>",
      null,
      stream("", Buffer.from(CONTENT("Page one text", 760), "latin1")),
      `<< /Type /Page /MediaBox ${A4} /Resources << /Font << /F1 1 0 R >> >> /Contents 3 0 R >>`,
      "<< /Type /Catalog /Pages 2 0 R >>",
    ];
    let current = 4;
    for (let index = 0; index < 5_000; index += 1) {
      objects.push(`<< /Type /Pages /Kids [${current} 0 R] /Count 1 >>`);
      current = objects.length;
    }
    objects[1] = `<< /Type /Pages /Kids [${current} 0 R] /Count 1 >>`;
    return file(objects, "5 0 R");
  },
  // Every level names its child twice, so the walk visits 2^20 pages. Measured, the reference does not
  // overflow and does not spend the visits' cost: it counts the million pages in a fraction of a
  // second and then refuses the request for naming more pages than one call takes (10 000). The
  // engine walks the same 2^depth nodes; `VOCABULARY.md` records both sides side by side.
  "a doubling page tree 20 levels deep: refused for its page count": () =>
    doublingPageTree(20),
  // An object stream whose every entry states the same offset: the reference parses the entry per
  // offset the file states and produces in a fraction of a second, so a file naming one body
  // thirty-two times is one the engine reads rather than refuses.
  "a container whose 32 entries state one offset": oneOffsetContainer,
  // A run of digits is a double to the reference, so two runs past a `u64` are two objects when they
  // round to two doubles and one when they round to the same: the row measures each side of that.
  "two numbers past a u64 that the reference keys apart": numbersKeyedApart,
  "a container naming the two numbers the reference keys apart": containerNamingNumbersKeyedApart,
  "a container naming one number in two runs of digits": containerNamingOneNumberInTwoRuns,
  // One number stated twice, the second body unparsable: the reference keeps the carried bytes at the
  // number and drops the first body, which is what the engine reports.
  "a number past a u64 replaced by a body that does not parse": carriedBodyReplacingANumber,
  // Both sides refuse the first, and both fault on the second, as the corpus records: a typed lookup
  // that finds a reference. The box is read only by the resize, so that is the request measured.
  "a /Kids stated as a reference to a reference": kidsByReference,
  "a /MediaBox stated as a reference to a reference": {
    build: mediaBoxByReference,
    request: { op: "pdf_resize", width: 400, height: 500, pages: "all" },
  },
  // The search for a free number: 3 200 objects numbered past the model, beside a reference naming
  // 4294967295. The reference produces in well under a second.
  "3 200 objects past the model beside a reference naming 4294967295": () =>
    manyPastModel(3_200),
  // A page beside streams that state a length, carry forty bytes and never close. The reference turns
  // the page in a fraction of a second, so a file this damaged is not one the engine may refuse; the
  // second row repeats the shape four times over, and what it adds is the reference's own cost growing
  // with the count.
  "a file with 100 streams that never end: the page turned": () =>
    damagedStreams(100),
  "a file with 400 streams that never end: the cost grows with them": () =>
    damagedStreams(400),
  // A page whose `/CropBox` is null: the reference reads the entry as "no crop" and falls back to the
  // media box, so the crop produces over the area asked for. The area is a fraction of the page, as
  // the contract states, which is why the numbers here are not points.
  "a page whose /CropBox is null: the crop falls back to the media box": {
    build: cropBoxNull,
    request: {
      op: "pdf_crop",
      area: { x: 0, y: 0, width: 0.5, height: 0.5 },
      pages: "all",
    },
  },
  // A page whose `/Resources` is a reference that names a reference: the reference's typed lookup
  // takes one step and finds a reference, so the text request faults with `Expected instance of
  // PDFDict, but got instance of PDFRef` rather than reading the page.
  "a /Resources stated as a reference to a reference": {
    build: resourcesByReference,
    request: { op: "pdf_text", text: "x", pages: "all" },
  },
  // A catalog whose `/AcroForm` is a reference that names a reference: the same one-step lookup, so
  // the fill faults with the same message rather than reaching the field it holds.
  "an /AcroForm stated as a reference to a reference": {
    build: acroFormByReference,
    request: { op: "pdf_form_fill", values: { name: "x" }, pages: "all" },
  },
  // A field's `/Parent` and a form's `/Fields` entry stated as references that name a reference: the
  // same one-step lookup, taken while the field tree is built, so the fill faults on the entry rather
  // than filling the field it names.
  "a field /Parent stated as a reference to a reference": {
    build: fieldParentByReference,
    request: { op: "pdf_form_fill", values: { name: "x" }, pages: "all" },
  },
  "a /Fields entry stated as a reference to a reference": {
    build: fieldsByReference,
    request: { op: "pdf_form_fill", values: { name: "x" }, pages: "all" },
  },
  // The name a field is addressed by comes from the chain its `/Parent` states, not from the tree the
  // form reaches through `/Kids`: a nameless child under `P` is `P.undefined`, and the reference fills
  // it under that name. The second row is the other half — a `/T` that is not a string, on a field the
  // request never reaches, which the reference never reads and so never throws on.
  "a field named through the chain its /Parent states": {
    build: fieldsNamedThroughTheirChain,
    request: { op: "pdf_form_fill", values: { "P.undefined": "x" }, pages: "all" },
  },
  "a /T that is not a string after the field the request names": {
    build: fieldNameThatIsNotAString,
    request: { op: "pdf_form_fill", values: { f0: "x" }, pages: "all" },
  },
  // `/Ff` is read only for the types whose kind it decides and by a text field's appearance, so a
  // signature whose flags are a reference to a reference is no fault: the field is simply not one a
  // fill can take a value for.
  "a signature whose /Ff is a reference to a reference": {
    build: signatureWhoseFlagsAreAReference,
    request: { op: "pdf_form_fill", values: { f0: "x" }, pages: "all" },
  },
  // One step of the reference's lookup, taken on the two arrays the field tree is built from: a
  // `/Fields` or a `/Kids` entry that names an array of its own is the same list, so the fields it
  // holds are filled.
  "a /Fields entry that names the array": {
    build: fieldsIndirectly,
    request: { op: "pdf_form_fill", values: { name: "x" }, pages: "all" },
  },
  "a /Kids entry that names the array": {
    build: kidsIndirectly,
    request: { op: "pdf_form_fill", values: { "P.Q": "x" }, pages: "all" },
  },
  // `createPDFAcroFields` keeps a kid only when the array states a reference that names a dictionary,
  // so a kid written out where it stands is no field of the reference's: it has none to fill.
  "a /Kids entry that is a field dictionary in place": {
    build: kidInPlace,
    request: { op: "pdf_form_fill", values: { "P.Q": "x" }, pages: "all" },
  },
  // `getInheritableAttribute` skips a stated null, so a field whose own `/FT` is null takes the type
  // its parent states and is a field of that kind.
  "a /FT null under a parent stating /FT": {
    build: nullTypeInherited,
    request: { op: "pdf_form_fill", values: { "P.Q": "x" }, pages: "all" },
  },
  // `T()` is a `lookupMaybe`, which answers undefined for a reference that names nothing: the field is
  // named by the chain's other links and filled under that name.
  "a /T stated as a reference that names nothing": {
    build: nameNamesNothing,
    request: { op: "pdf_form_fill", values: { "P.undefined": "x" }, pages: "all" },
  },
  // A text field's `/Ff` is read where its appearance is regenerated: the field the request does not
  // fill keeps the appearance the file states, so its flags are never read and the form produces.
  "a clean text field whose /Ff is a reference": {
    build: cleanFieldWhoseFlagsAreAReference,
    request: { op: "pdf_form_fill", values: { f0: "x" }, pages: "all" },
  },
  // The other half: the field the request fills has its appearance regenerated, and that read of `/Ff`
  // is the fault.
  "a filled text field whose /Ff is a reference": {
    build: filledFieldWhoseFlagsAreAReference,
    request: { op: "pdf_form_fill", values: { f0: "x" }, pages: "all" },
  },
  // `getWidgets` answers no widget for an empty `/Kids` array, so the value is written and no
  // appearance is.
  "a text field whose /Kids is an empty array": {
    build: emptyKids,
    request: { op: "pdf_form_fill", values: { f0: "x" }, pages: "all" },
  },
  // The typed widget read throws on a kid that is not a dictionary, for every kind whose appearance
  // the fill regenerates.
  "a text field whose /Kids states a number": {
    build: kidIsANumber,
    request: { op: "pdf_form_fill", values: { f0: "x" }, pages: "all" },
  },
  // A `/Ff` stated as a reference that names nothing is no flags at all: `Ff()` is a `lookupMaybe`.
  "a choice field whose /Ff names nothing": {
    build: choiceWhoseFlagsNameNothing,
    request: { op: "pdf_form_fill", values: { f0: "a" }, pages: "all" },
  },
  // Flags are a 32-bit integer operation in the reference: `hasFlag` tests `getFlags() & flag`, and
  // JavaScript's `&` converts the number to a signed 32-bit integer. A negative real sets every bit,
  // so the choice field is a combo box (it produces) and the button is a push button (it refuses); a
  // real past 2^32 keeps no bit the kind reads, so the choice field is a list box (it refuses); and a
  // whole number is read through the double its digits state, so 2^53 + 131071 rounds onto the combo
  // bit. The last row is the model's own band: `4294967295.5` is a double there and a 32-bit float
  // here, so the reference produces a combo box where the engine refuses a list box (`PARSER.md`
  // §5.16).
  "a choice field whose flags are a negative real": {
    build: choiceFlagsNegativeReal,
    request: { op: "pdf_form_fill", values: { f0: "x" }, pages: "all" },
  },
  "a button whose flags are a negative real": {
    build: buttonFlagsNegativeReal,
    request: { op: "pdf_form_fill", values: { f0: "x" }, pages: "all" },
  },
  "a choice field whose flags are a huge real": {
    build: choiceFlagsHugeReal,
    request: { op: "pdf_form_fill", values: { f0: "x" }, pages: "all" },
  },
  "a choice field whose flags are a real past the float": {
    build: choiceFlagsPastTheFloat,
    request: { op: "pdf_form_fill", values: { f0: "x" }, pages: "all" },
  },
  "a choice field whose flags are a whole number past 2^53": {
    build: choiceFlagsPastTheExactInteger,
    request: { op: "pdf_form_fill", values: { f0: "x" }, pages: "all" },
  },
  // A field whose kid list or parent link returns to itself: `getFields` and `getFullyQualifiedName`
  // recurse on the tree and the link, so the reference dies with `Maximum call stack size exceeded`.
  // The engine's walk keeps the chain it is inside and answers; the stop is its own bound, and this
  // measured failure is what justifies it (`PARSER.md` §5.17).
  "a form whose field kid list returns to its own container": {
    build: fieldKidsCycle,
    request: { op: "pdf_form_fill", values: { "P.Q": "x" }, pages: "all" },
  },
  "a form whose field parent link returns to itself": {
    build: fieldParentCycle,
    request: { op: "pdf_form_fill", values: { P: "x" }, pages: "all" },
  },
  // `/MaxLen` is a `lookupMaybe(…, PDFNumber)`, so a real is a length the reference enforces and
  // prints; the engine reads only a whole number and writes the value (`PARSER.md` §5.16).
  "a text field whose /MaxLen is a real": {
    build: textMaxLenWholeReal,
    request: { op: "pdf_form_fill", values: { f0: "abcdef" }, pages: "all" },
  },
  "a text field whose /MaxLen is a fraction": {
    build: textMaxLenFraction,
    request: { op: "pdf_form_fill", values: { f0: "abcdef" }, pages: "all" },
  },
  // A `/Parent` stated as a dictionary in place is followed by the constructor's `ascend`, so the field
  // inherits the parent's type and is named by its own `/T`.
  "a field whose /Parent is a dictionary in place": {
    build: parentInPlace,
    request: { op: "pdf_form_fill", values: { Q: "x" }, pages: "all" },
  },
  // The widget list is read after the values are applied, so a malformed one does not turn the usage
  // refusal a missing field is into a fault.
  "a bad widget kid beside a request naming no field": {
    build: missingFieldBesideABadKid,
    request: { op: "pdf_form_fill", values: { zzz: "x" }, pages: "all" },
  },
  // `Kids()` is a typed `lookupMaybe`, so an entry that is not an array throws where the list is read.
  "a text field whose /Kids is a number": {
    build: kidsIsANumber,
    request: { op: "pdf_form_fill", values: { f0: "x" }, pages: "all" },
  },
  // `/First` is a position the entry's own offset is added to, so a negative one reads the object its
  // offsets reach and only a fraction names no position at all.
  "a container stating /First 12 places its entry": () => containerLocating(12, 0, 12),
  "a container stating /First -4 whose offsets reach 16": () => containerLocating(-4, 20, 16),
  "a container stating /First 12.5, which names no position": () =>
    containerLocating("12.5", 0, 12),
  // A page beside 3 200 streams that never close and one `endstream` at the end of the file: every
  // recovery finds its keyword only at the end, so this is the shape whose recovery is quadratic in
  // the streams. The count is the ceiling the probe keeps under so a run stays in a few minutes.
  "3 200 streams that never end beside one endstream": () =>
    streamsBeforeOneEnd(3_200),
};

const run = (name, probe, directory) => {
  // A row is a builder, or a builder beside the request it is measured with when the shape is only a
  // fault on one operation.
  const { build, request: extra = {} } =
    typeof probe === "function" ? { build: probe } : probe;
  const input = join(directory, "input.pdf");
  const output = join(directory, "output.pdf");
  const request = join(directory, "request.json");
  const result = join(directory, "result.json");
  for (const path of [output, result]) rmSync(path, { force: true });
  writeFileSync(input, build());
  writeFileSync(
    request,
    JSON.stringify({
      op: "pdf_rotate",
      degrees: 90,
      pages: "all",
      ...extra,
      input,
      output,
      font: FONT,
      result,
    }),
  );
  const started = process.hrtime.bigint();
  const kit = spawnSync(BUN, ["--no-install", "--no-env-file", KIT, request], {
    cwd: dirname(KIT),
    timeout: 120_000,
  });
  const elapsed = Number(process.hrtime.bigint() - started) / 1e9;
  let answer = null;
  try {
    answer = JSON.parse(readFileSync(result, "utf8"));
  } catch {
    answer = null;
  }
  let written = 0;
  try {
    written = readFileSync(output).length;
  } catch {
    written = 0;
  }
  const stderr = String(kit.stderr || "").trim();
  const notes = (stderr.split("\n")[0] || "").replace(/\s+/g, " ").trim().slice(0, 60);
  const verdict = answer === null ? "no result" : answer.ok ? "produced" : answer.error;
  return { name, verdict, elapsed, written, notes };
};

const directory = mkdtempSync(join(tmpdir(), "probe-pdf-bounds-"));
const rows = [];
try {
  for (const [name, build] of Object.entries(probes)) rows.push(run(name, build, directory));
} finally {
  rmSync(directory, { recursive: true, force: true });
}
const width = Math.max(...rows.map((row) => row.name.length));
for (const row of rows) {
  console.log(
    `${row.name.padEnd(width)}  ${String(row.verdict).padEnd(24)}  ${row.elapsed.toFixed(2)} s  ${row.written} bytes  ${row.notes}`,
  );
}

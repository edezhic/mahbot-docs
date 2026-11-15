#!/usr/bin/env bun
// Derives the pinned `docx` layout's width and constant tables into the JSON the Rust
// layout port reads.
//
// The package's bundle is not ours to edit, and its tables are module-scope `var`s, so the export
// statement they need is appended to a temporary copy. Everything is built in memory and checked
// before the first file is written: a package that has moved on must not leave half a data set.

import {
  copyFileSync,
  existsSync,
  mkdirSync,
  mkdtempSync,
  readFileSync,
  rmSync,
  writeFileSync,
} from "node:fs";
import { homedir, tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "../..");
const OUTPUT = join(ROOT, "document-engine/assets/docx-layout");
const BUNDLE = "dist/layout.mjs";

const fail = (message) => {
  console.error(`derive-docx-layout-data: ${message}`);
  process.exit(1);
};

// ── the package ───────────────────────────────────────────────────────────

const findPackage = () => {
  const candidates = [];
  if (process.env.PARITY_DOCX) candidates.push(resolve(process.env.PARITY_DOCX));
  candidates.push(join(homedir(), ".bun/install/cache/docx@9.12.0@@@1"));
  candidates.push(join(ROOT, "node_modules/docx"));
  for (const directory of candidates) {
    if (existsSync(join(directory, BUNDLE))) return directory;
  }
  fail(
    [
      "cannot find the docx package: none of these holds " + BUNDLE,
      ...candidates.map((directory) => `  ${directory}`),
      "set PARITY_DOCX to the package directory, or install docx 9.12.0 under node_modules.",
    ].join("\n"),
  );
};

// The bindings the data comes from, in the order the files below name them.
const BINDINGS = [
  "FONT_WIDTH_RANGES",
  "MORE_WIDTH_RANGES",
  "FALLBACK_FACES",
  "FALLBACK_FONTS",
  "FONT_WIDTHS",
  "SAME_WIDTHS",
  "SIMILAR_FONTS",
  "MORE_WIDTHS",
  "ARABIC_LETTERS",
  "LAM_ALEFS",
  "ARABIC_WIDTHS",
  "EAST_ASIAN",
  "HANGUL",
  "EAST_ASIAN_FONTS",
];

// ── the module list ───────────────────────────────────────────────────────

// The module list the crate ships is the one home of what the bundle must carry: the thirty source
// modules, in bundle order, and the seven runtime helpers. This script compares the bundle it is
// about to derive from against that file in both directions, so a package that has moved on fails
// here rather than writing a list nobody reviewed — and the list is not stated twice.
const COMMITTED = join(dirname(fileURLToPath(import.meta.url)), "../../document-engine/assets/docx-layout");
const committedModules = JSON.parse(readFileSync(join(COMMITTED, "modules.json"), "utf8"));
const EXPECTED_MODULES = committedModules.modules;
const EXPECTED_RUNTIME = committedModules.runtime_markers;

const splitRegions = (source) => {
  const regions = [];
  let current;
  for (const line of source.split("\n")) {
    const marker = /^\/\/#region (.*)$/.exec(line);
    if (marker) {
      current = { name: marker[1].trim(), lines: [] };
      regions.push(current);
    } else if (current) {
      current.lines.push(line);
    }
  }
  return regions;
};

const checkModuleList = (regions) => {
  const modules = regions
    .map(({ name }) => name)
    .filter((name) => name.startsWith("src/"));
  const runtime = regions
    .map(({ name }) => name)
    .filter((name) => name.startsWith("\\0@oxc-project+runtime@"));
  const other = regions
    .map(({ name }) => name)
    .filter((name) => !modules.includes(name) && !runtime.includes(name));
  if (other.length > 0) {
    fail(`the bundle names a region that is neither a source module nor a runtime helper: ${other.join(", ")}`);
  }
  const missing = modules.filter((name) => !EXPECTED_MODULES.includes(name));
  if (missing.length > 0) {
    fail(
      `the bundle names a source module the committed list does not: ${missing.join(", ")} — review it, then add it to assets/docx-layout/modules.json and the disposition table`,
    );
  }
  const stale = EXPECTED_MODULES.filter((name) => !modules.includes(name));
  if (stale.length > 0) {
    fail(`the committed list names a source module the bundle no longer carries: ${stale.join(", ")}`);
  }
  if (JSON.stringify(modules) !== JSON.stringify(EXPECTED_MODULES)) {
    fail("the source modules are not in the order the list states");
  }
  const missingRuntime = runtime.filter((name) => !EXPECTED_RUNTIME.includes(name));
  const staleRuntime = EXPECTED_RUNTIME.filter((name) => !runtime.includes(name));
  if (missingRuntime.length > 0 || staleRuntime.length > 0) {
    fail(
      `the runtime helpers differ from the list: ${[...missingRuntime, ...staleRuntime].join(", ")}`,
    );
  }
  return { modules, runtime };
};

// ── the constants ────────────────────────────────────────────────────────

// The constants the port reads. A constant the bundle states and this list omits is not shipped, and
// one this list names and the bundle drops fails the derivation: the data is what the port reads, so
// a constant nothing reads has no place in it. The bundle states the distance between automatic tab
// stops four times, under the bundler's own names; the port reads it once, as `TAB_STOP`, so the list
// names it once.
const READ_CONSTANTS = [
  "EIGHTHS_PER_POINT",
  "FIFTIETHS_OF_A_PERCENT",
  "PASSES",
  "TAB_STOP",
  "TOLERANCE",
  "WORD_2007_MODE",
  "WORD_2010_MODE",
];

// What each constant counts, and in what unit. A number in the bundle without a row here is a
// constant this script has not been taught, and it fails rather than inventing a meaning. The unit is
// what the restatement scan reads: a numeral that coincides with a vocabulary bound of another unit
// is not a restatement of it.
const ANNOTATIONS = {
  EIGHTHS_PER_POINT: ["eighths of a point a border width is stated in", "eighths of a point"],
  FIFTIETHS_OF_A_PERCENT: [
    "fiftieths of a percent a table's width is stated in",
    "fiftieths of a percent",
  ],
  PASSES: ["layout passes made while page numbers change how lines wrap", "passes"],
  TAB_STOP: ["distance between automatic tab stops", "points"],
  TOLERANCE: ["difference between two lengths below which they count as equal", "points"],
  WORD_2007_MODE: [
    "the compatibility mode a document that states none is laid out in",
    "Word compatibility mode",
  ],
  WORD_2010_MODE: [
    "the compatibility mode at which Word breaks lines between Chinese and Japanese characters",
    "Word compatibility mode",
  ],
};

const numeric = /^\s*var\s+([A-Za-z0-9_$]+)\s*=\s*(-?(?:\d+\.?\d*|\.\d+)(?:[eE][+-]?\d+)?)\s*;/;

const readConstants = (regions) => {
  const found = new Map();
  for (const { name: module, lines } of regions) {
    for (const line of lines) {
      const match = numeric.exec(line);
      if (!match) continue;
      // `$1`, `$2` are the bundler's aliases of one module-scope name, not different constants.
      const name = match[1].replace(/\$\d+$/, "");
      const value = Number(match[2]);
      const entry = found.get(name);
      if (entry === undefined) {
        found.set(name, { value, modules: [module] });
      } else if (entry.value !== value) {
        fail(`the bundle states ${name} as both ${entry.value} and ${value}`);
      } else if (!entry.modules.includes(module)) {
        entry.modules.push(module);
      }
    }
  }

  const selected = new Map();
  for (const [name, entry] of found) {
    if (READ_CONSTANTS.includes(name)) {
      selected.set(name, entry);
    }
  }
  for (const name of READ_CONSTANTS) {
    if (!selected.has(name)) fail(`the bundle no longer states ${name}`);
  }

  const constants = {};
  for (const name of [...selected.keys()].sort()) {
    const annotation = ANNOTATIONS[name];
    if (annotation === undefined) {
      fail(`${name} is a constant this script has no quantity for; add it to ANNOTATIONS`);
    }
    const [quantity, unit] = annotation;
    // The value, what it counts and its unit: the modules that state it are the bundle's own
    // business, and the crate reads none of them.
    constants[name] = { value: selected.get(name).value, quantity, unit };
  }
  const unselected = Object.keys(ANNOTATIONS).filter((name) => !selected.has(name));
  if (unselected.length > 0) {
    fail(`ANNOTATIONS names constants the bundle no longer states: ${unselected.join(", ")}`);
  }
  return constants;
};

// ── the tables ───────────────────────────────────────────────────────────

const expectKeys = (object, allowed, where) => {
  for (const key of Object.keys(object)) {
    if (!allowed.includes(key)) fail(`${where} states a field this script does not know: ${key}`);
  }
};

const WIDTH_FACE_KEYS = [
  "name", "lineHeight", "descent", "regular", "bold", "italic", "boldItalic", "ligatures", "notKernedWithLigatures",
];
const MORE_WIDTH_FACE_KEYS = ["name", "regular", "bold", "italic", "boldItalic"];
const CLASS_NAMES = [["regular", "regular"], ["bold", "bold"], ["italic", "italic"], ["boldItalic", "bold_italic"]];

const widthFace = (face) => {
  expectKeys(face, WIDTH_FACE_KEYS, `the width face ${face.name}`);
  const out = { name: face.name, line_height: face.lineHeight, descent: face.descent, regular: face.regular, bold: face.bold };
  if (face.italic !== undefined) out.italic = face.italic;
  if (face.boldItalic !== undefined) out.bold_italic = face.boldItalic;
  if (face.ligatures !== undefined) out.ligatures = face.ligatures;
  if (face.notKernedWithLigatures !== undefined) out.not_kerned_with_ligatures = face.notKernedWithLigatures;
  return out;
};

const moreWidthsFace = (face) => {
  expectKeys(face, MORE_WIDTH_FACE_KEYS, `the supplementary width face ${face.name}`);
  const classes = {};
  for (const [source, name] of CLASS_NAMES) {
    const data = face[source];
    if (data === undefined) continue;
    expectKeys(data, ["widths", "fonts"], `the ${source} class of ${face.name}`);
    if (data.widths === undefined || data.fonts === undefined) {
      fail(`the ${source} class of ${face.name} no longer states its widths and fonts`);
    }
    classes[name] = { widths: data.widths, fonts: data.fonts };
  }
  return { name: face.name, classes };
};

const arabicFace = (face) => {
  expectKeys(face, ["name", "regular", "bold", "drawnIn"], `the Arabic width face ${face.name}`);
  if (face.regular === undefined || face.bold === undefined) {
    fail(`the Arabic width face ${face.name} no longer states its regular and bold widths`);
  }
  const out = { name: face.name, regular: face.regular, bold: face.bold };
  if (face.drawnIn !== undefined) out.drawn_in = face.drawnIn;
  return out;
};

// A regex pair is an ordered rule, so it stays an array; the source and flags are what the bundle
// states, and the first rule that matches wins.
const fontRules = (rules) => rules.map(([pattern, face]) => ({ pattern: pattern.source, flags: pattern.flags, face }));

// ── main ─────────────────────────────────────────────────────────────────

const packageDirectory = findPackage();
// The data's shape is the 9.12.0 bundle's; another version is a deliberate re-derivation, not a run.
const manifest = JSON.parse(readFileSync(join(packageDirectory, "package.json"), "utf8"));
if (manifest.name !== "docx" || manifest.version !== "9.12.0") {
  fail(`the package is ${manifest.name}@${manifest.version}, not docx@9.12.0`);
}
const source = readFileSync(join(packageDirectory, BUNDLE), "utf8");
// Checked before the export is appended: a name the bundle no longer declares would otherwise fail
// as a module-linking error rather than as the missing table it is.
const undeclared = BINDINGS.filter(
  (name) => !new RegExp(`(?:^|\\n)\\s*(?:var|let|const|function|class)\\s+${name}\\b`).test(source),
);
if (undeclared.length > 0) fail(`the bundle no longer states: ${undeclared.join(", ")}`);
const regions = splitRegions(source);
const { modules, runtime } = checkModuleList(regions);
const constants = readConstants(regions);

const scratch = mkdtempSync(join(tmpdir(), "docx-layout-data-"));
let tables;
try {
  const copy = join(scratch, "layout-data.mjs");
  writeFileSync(copy, `${source}\nexport { ${BINDINGS.join(", ")} };\n`);
  tables = await import(pathToFileURL(copy).href);
} finally {
  rmSync(scratch, { recursive: true, force: true });
}
// A table declared but left undefined is as unusable as one the bundle dropped.
const undefinedBindings = BINDINGS.filter((name) => tables[name] === undefined);
if (undefinedBindings.length > 0) fail(`the bundle no longer states: ${undefinedBindings.join(", ")}`);

const widths = {
  ranges: tables.FONT_WIDTH_RANGES,
  more_ranges: tables.MORE_WIDTH_RANGES,
  fallback_faces: tables.FALLBACK_FACES,
  faces: tables.FONT_WIDTHS.map(widthFace),
  same_widths: fontRules(tables.SAME_WIDTHS),
  similar_fonts: fontRules(tables.SIMILAR_FONTS),
};

const moreWidths = { faces: tables.MORE_WIDTHS.map(moreWidthsFace), fallback_fonts: tables.FALLBACK_FONTS };

const arabicWidths = {
  letters: tables.ARABIC_LETTERS,
  lam_alefs: tables.LAM_ALEFS,
  faces: tables.ARABIC_WIDTHS.map(arabicFace),
};

const moduleList = { package: manifest.name, version: manifest.version, modules, runtime_markers: runtime };

// ── the character classes the line breaker reads ──────────────────────────
// The reference tests a character against three regular expressions: `\p{M}` (a mark that belongs to
// the character before it), and the `EAST_ASIAN`/`HANGUL` script classes. Their code-point ranges are
// enumerated here rather than restated in Rust, so the port cannot drift from the expression the
// pinned library evaluates.
const rangesOf = (test) => {
  const ranges = [];
  let start;
  for (let code = 0; code <= 0x10ffff; code += 1) {
    const inside = test(String.fromCodePoint(code));
    if (inside && start === undefined) start = code;
    if (!inside && start !== undefined) {
      ranges.push([start, code - 1]);
      start = undefined;
    }
  }
  if (start !== undefined) ranges.push([start, 0x10ffff]);
  return ranges;
};
const unicode = {
  marks: rangesOf((character) => /\p{M}/u.test(character)),
  east_asian: rangesOf((character) => tables.EAST_ASIAN.test(character)),
  hangul: rangesOf((character) => tables.HANGUL.test(character)),
};

// The East Asian faces the layout measures Chinese, Japanese and Korean text in: their own line
// metrics, whether their characters are an em or half an em wide, and the face their Latin letters
// are measured as. A created document's runs name none, so the layout draws their East Asian text in
// the fallback face the reference states.
const eastAsian = { faces: tables.EAST_ASIAN_FONTS };

const files = {
  "widths.json": widths,
  "more_widths.json": moreWidths,
  "arabic_widths.json": arabicWidths,
  "constants.json": constants,
  "modules.json": moduleList,
  "unicode.json": unicode,
  "east_asian.json": eastAsian,
};

const contents = Object.entries(files).map(([name, value]) => [name, `${JSON.stringify(value, null, 2)}\n`]);

mkdirSync(OUTPUT, { recursive: true });
for (const [name, content] of contents) writeFileSync(join(OUTPUT, name), content);
copyFileSync(join(packageDirectory, "LICENSE"), join(OUTPUT, "LICENSE"));
console.log(`wrote ${contents.length + 1} files to ${OUTPUT}`);

// Whether a created document's runs ask for kerning, and whether the reference measures its text
// through the shaped path at all.
// The disposition table's rows for `font-kerning.ts` and `kerning.ts` rest on this: a created
// document states no `w:kern` and no ligature setting, so `measureTextWidthAsDrawn` is
// `measureTextWidth` and the kerning and ligature tables are never consulted.
import { mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { pathToFileURL } from "node:url";
import { documentOf, packageDir, paragraph, readParts, report } from "./lib.mjs";

const text = "Hello world";
const parts = await readParts(documentOf([paragraph(text)]));
const body = Buffer.from(parts["word/document.xml"]).toString("utf8");

// The bundle exports only the layout entry points, so a temporary copy carries the two measurers
// out: they are the functions a run's width is worked out with.
const scratch = mkdtempSync(join(tmpdir(), "docx-kerning-probe-"));
let measured;
try {
  const copy = join(scratch, "layout.mjs");
  writeFileSync(
    copy,
    `${readFileSync(join(packageDir, "dist/layout.mjs"), "utf8")}\nexport { measureTextWidth, measureTextWidthAsDrawn };\n`,
  );
  measured = await import(pathToFileURL(copy).href);
} finally {
  rmSync(scratch, { recursive: true, force: true });
}

const unshaped = measured.measureTextWidth(text);
const asDrawn = measured.measureTextWidthAsDrawn(text);
report("kerning", {
  // No run in a created document states a kerning or ligature setting, so the layout's measurer
  // never enters the shaping path and the kerning and ligature tables are never read.
  runsAskingForKerning: (body.match(/<w:kern\b/g) ?? []).length,
  noRunAsksForKerning: (body.match(/<w:kern\b/g) ?? []).length === 0,
  unshapedWidth: unshaped,
  asDrawnWidth: asDrawn,
  asDrawnEqualsUnshaped: asDrawn === unshaped,
});

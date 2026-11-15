// Which faces a created document's text is measured in, and whether any font file is read.
// The disposition table's rows for `office-cloud-fonts.ts`, `font-file.ts` and `measure-width.ts`
// rest on this: a created document names no font of its own, so no run names a cloud font, none is
// embedded, and the measurer is the width tables' own rather than one a canvas would give.
import { documentOf, paragraph, readParts, report } from "./lib.mjs";

const parts = await readParts(documentOf([paragraph("Hello world"), paragraph("Здравствуйте")]));
const body = Buffer.from(parts["word/document.xml"]).toString("utf8");
report("fonts", {
  // No run in a created document states a face or a size, and the package embeds no font: the
  // library's measurer falls back to Times New Roman at ten points for every run, and its
  // East Asian and embedded-font tables are never consulted.
  runsNamingAFace: (body.match(/<w:rFonts\b/g) ?? []).length,
  runsNamingASize: (body.match(/<w:sz\b/g) ?? []).length,
  embeddedFonts: Object.keys(parts).filter((name) => name.startsWith("word/fonts/")).length,
  // `measure-width.ts` measures through a canvas, which this environment has none of: its measurer
  // cannot run here at all, whatever a caller asks for.
  hasCanvas: typeof OffscreenCanvas !== "undefined" || typeof document !== "undefined",
});

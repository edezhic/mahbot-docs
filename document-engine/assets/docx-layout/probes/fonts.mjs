// Which faces a created document's text is measured in, and whether any font file is read.
// The disposition table's rows for `text-styles.ts`, `office-cloud-fonts.ts`, `font-file.ts` and
// `measure-width.ts` rest on this: a created document names no face of its own and embeds no font, so
// its text measures in the width tables' default face, while its styles part states the heading sizes
// the port measures a heading at, and no canvas is available to measure through.
import { documentOf, heading, paragraph, readParts, report } from "./lib.mjs";

const parts = await readParts(documentOf([
  paragraph("Hello world"),
  paragraph("Здравствуйте"),
  heading("A heading", 1),
]));
const body = Buffer.from(parts["word/document.xml"]).toString("utf8");
const styles = Buffer.from(parts["word/styles.xml"]).toString("utf8");
// The size each heading style states, in points: `w:sz` states half-points.
const headingSizes = Object.fromEntries(
  [...styles.matchAll(/<w:style\b[^>]*w:styleId="(Heading\d)"[^>]*>([\s\S]*?)<\/w:style>/g)]
    .map(([, id, definition]) => {
      const half = /<w:sz w:val="(\d+)"\/>/.exec(definition)?.[1];
      return [id, half === undefined ? null : Number(half) / 2];
    })
    .filter(([, size]) => size !== null),
);
report("fonts", {
  // No run in a created document's body states a face or a size; the styles part states the heading
  // sizes, which the layout reads from the paragraph style chain.
  runsNamingAFace: (body.match(/<w:rFonts\b/g) ?? []).length,
  runsNamingASize: (body.match(/<w:sz\b/g) ?? []).length,
  headingSizes,
  embeddedFonts: Object.keys(parts).filter((name) => name.startsWith("word/fonts/")).length,
  // `measure-width.ts` measures through a canvas, which this environment has none of: its measurer
  // cannot run here at all, whatever a caller asks for.
  hasCanvas: typeof OffscreenCanvas !== "undefined" || typeof document !== "undefined",
});

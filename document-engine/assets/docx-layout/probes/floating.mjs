// Whether a created document's picture is a floating drawing, and whether a paragraph carries a frame.
// The disposition table's rows for `floating-drawings.ts` and `text-frames.ts` rest on this.
import { docx, documentOf, paragraph, readParts, report } from "./lib.mjs";

const picture = new docx.ImageRun({
  type: "png",
  data: Buffer.from(
    "iVBORw0KGgoAAAANSUhEUgAAAAQAAAACCAIAAADwyuo0AAAAEklEQVR42mP8z8Dwn4GBgYGJAQoAHgQCAZ7nW6kAAAAASUVORK5CYII=",
    "base64",
  ),
  transformation: { width: 120, height: 60 },
});
const parts = await readParts(documentOf([paragraph("Before"), new docx.Paragraph({ children: [picture] })]));
const body = Buffer.from(parts["word/document.xml"]).toString("utf8");
report("floating", {
  // The picture is a `wp:inline` drawing, which the line it stands in carries: no text flows around
  // it, and no paragraph in the document carries a `w:framePr`.
  inlineDrawings: (body.match(/<wp:inline\b/g) ?? []).length,
  floatingDrawings: (body.match(/<wp:anchor\b/g) ?? []).length,
  frames: (body.match(/<w:framePr\b/g) ?? []).length,
  // The drawing states its own size, which is what the layout reads: it never works one out from the
  // image file's pixels and density.
  statedExtents: (body.match(/<wp:extent\b[^>]*\bcx="[^"]*"[^>]*\bcy="[^"]*"/g) ?? []).length,
});

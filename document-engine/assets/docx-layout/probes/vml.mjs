// Whether a created document writes any VML.
// The disposition table's row for `vml-drawings.ts` rests on this.
import { docx, documentOf, readParts, report } from "./lib.mjs";

const picture = new docx.ImageRun({
  type: "png",
  data: Buffer.from(
    "iVBORw0KGgoAAAANSUhEUgAAAAQAAAACCAIAAADwyuo0AAAAEklEQVR42mP8z8Dwn4GBgYGJAQoAHgQCAZ7nW6kAAAAASUVORK5CYII=",
    "base64",
  ),
  transformation: { width: 60, height: 30 },
});
const document = documentOf([new docx.Paragraph({ children: [picture] })]);
const parts = await readParts(document);
const body = Buffer.from(parts["word/document.xml"]).toString("utf8");
report("vml", {
  // A created document writes DrawingML only: its picture is a `w:drawing`, and the part holds no
  // `w:pict` element for the VML reader to walk.
  pictElements: (body.match(/<w:pict/g) ?? []).length,
  drawingElements: (body.match(/<w:drawing/g) ?? []).length,
});

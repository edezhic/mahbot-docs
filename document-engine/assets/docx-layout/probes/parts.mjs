// The parts a created package holds, and the elements its body carries.
// The disposition table's rows for `bound-controls.ts`, `read-document.ts`, `equations.ts` and
// `imported-documents.ts` rest on this.
import { docx, documentOf, heading, paragraph, readParts, report } from "./lib.mjs";

// A table of contents is a `w:sdt` — the one shape a created body carries that a content control is
// also written as — so the document below names one, and the probe reads what that control states.
const contents = new docx.TableOfContents("", { hyperlink: true, headingStyleRange: "1-3" });
const parts = await readParts(documentOf([contents, heading("One"), paragraph("Text.")]));
const body = Buffer.from(parts["word/document.xml"]).toString("utf8");
report("parts", {
  // The set of parts a created document carries: nothing imports another document, so the package
  // holds no `altChunk` and no custom XML part, and the body holds no text box or equation.
  parts: Object.keys(parts).sort(),
  bodyElements: ["w:txbxContent", "w:sdt", "m:oMath", "w:altChunk"].filter((name) =>
    body.includes(`<${name}`),
  ),
  // The contents control is a `w:sdt` that binds nothing: no `w:dataBinding`, and no custom XML part
  // for one to name, which is what `bound-controls.ts` fills.
  dataBindings: (body.match(/<w:dataBinding\b/g) ?? []).length,
  customXmlParts: Object.keys(parts).filter((name) => name.startsWith("customXml/")).length,
});

// The number formats a created document's numbering states, and whether its runs state a language.
// The disposition table's rows for `number-format.ts` and `line-break-rules.ts` rest on this: only a
// decimal list's marker carries a number, and no run names a language, so Word's kinsoku lists and
// its `anywhere` rule are never consulted.
import { docx, documentOf, readParts, report } from "./lib.mjs";

const document = documentOf([
  new docx.Paragraph({
    text: "One",
    numbering: { reference: "document-kit-ordered", level: 0 },
  }),
]);
const parts = await readParts(document);
const numbering = Buffer.from(parts["word/numbering.xml"]).toString("utf8");
const body = Buffer.from(parts["word/document.xml"]).toString("utf8");
report("numbering", {
  // The formats the numbering part states, and the level texts it substitutes a number into.
  formats: [...new Set([...numbering.matchAll(/<w:numFmt w:val="([^"]+)"/g)].map(([, format]) => format))].sort(),
  levelTexts: [...new Set([...numbering.matchAll(/<w:lvlText w:val="([^"]+)"/g)].map(([, text]) => text))],
  // A run that names a language would be one Word resolves a kinsoku list for.
  runsNamingALanguage: (body.match(/<w:lang\b/g) ?? []).length,
});

// The section, column and compatibility properties a created document states.
// The disposition table's row for `paginate.ts` rests on this: one section, one column, and the
// compatibility mode at which every older-mode rule is out of reach.
import { documentOf, paragraph, readParts, report } from "./lib.mjs";

const document = documentOf([paragraph("Text.")]);
const parts = await readParts(document);
const body = Buffer.from(parts["word/document.xml"]).toString("utf8");
const settings = Buffer.from(parts["word/settings.xml"]).toString("utf8");
report("sections", {
  sections: (body.match(/<w:sectPr/g) ?? []).length,
  columns: (body.match(/<w:cols\b/g) ?? []).length,
  compatibilityMode: /w:compatSetting[^>]*w:val="(\d+)"[^>]*w:name="compatibilityMode"/.exec(settings)?.[1] ?? null,
});

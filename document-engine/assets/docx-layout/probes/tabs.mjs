// Where a created document's contents entries put their page numbers, and what line-breaking rules
// it never asks for. The disposition table's row for `line-breaking.ts` rests on this: an entry
// states one right tab stop at the column's end, so the number ends on it and a fitting title costs
// one line, and no paragraph states justification, hyphenation or a character grid.
import { docx, documentOf, heading, layout, readParts, report } from "./lib.mjs";

const title = "A very long heading text that must wrap inside the table of contents entry because it is much wider than the column";
const document = documentOf([
  new docx.TableOfContents("", { hyperlink: true, headingStyleRange: "1-3" }),
  heading("Short", 1),
  heading(title, 1),
]);
const parts = await readParts(document);
const body = Buffer.from(parts["word/document.xml"]).toString("utf8");
const styles = Buffer.from(parts["word/styles.xml"]).toString("utf8");
// The entries' lines as the reference lays them out: the short title's entry is one line, and the
// long title's two, with the number after the tab on the title's last line.
const { pages, stoppedAt } = layout.layoutDocument(document);
const entryLines = pages
  .flatMap((page) => page.body)
  .filter((block) => block.type === "paragraph" && block.lines.some((line) => line.text.includes("\t")))
  .map((block) => block.lines.length);
report("tabs", {
  // The stops the entries state: one right stop each, at the column's end.
  tabStopAlignments: [...new Set([...body.matchAll(/<w:tab\b[^>]*w:val="([^"]+)"/g)].map(([, alignment]) => alignment))].sort(),
  tabStopPositions: [...new Set([...body.matchAll(/<w:tab\b[^>]*w:pos="([^"]+)"/g)].map(([, position]) => position))],
  // The rules a created document never asks for.
  paragraphsStatingJustification: (body.match(/<w:jc\b/g) ?? []).length
    + (styles.match(/<w:jc\b/g) ?? []).length,
  runsStatingHyphenation: (body.match(/<w:suppressAutoHyphens\b/g) ?? []).length
    + (styles.match(/<w:suppressAutoHyphens\b/g) ?? []).length,
  // The section states a `w:docGrid` of no type, which the reference reads as no grid.
  gridType: /<w:docGrid\b[^>]*w:type="([^"]+)"/.exec(body)?.[1] ?? null,
  entryLines,
  // Where the reference stopped, when it did: a run that stopped has line counts that say nothing.
  stoppedAt: stoppedAt ?? null,
});

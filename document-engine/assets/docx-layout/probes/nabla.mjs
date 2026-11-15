// The character that stops the layout, and the answer a stop gives.
// The disposition table's row for `cambria-math.ts` rests on this.
import { docx, documentOf, paragraph, readParts, report } from "./lib.mjs";

const counted = (content) =>
  documentOf(content, {
    footers: { default: new docx.Footer({ children: [new docx.Paragraph({ children: [new docx.TextRun({ children: [docx.PageNumber.TOTAL_PAGES] })] })] }) },
  });
const stopped = await readParts(counted([paragraph("\u2207")]));
const settles = await readParts(counted([paragraph("\u2211"), paragraph("\u222b")]));
const pageCountOf = (parts) => {
  const footer = Buffer.from(parts["word/footer1.xml"] ?? "").toString("utf8");
  return /<w:t[^>]*>(\d+)<\/w:t>/.exec(footer)?.[1] ?? null;
};
report("nabla", {
  // The summation and integral signs settle: their widths are in the measuring face's own table. The
  // nabla is the symbol that stops the layout, so the page count it would write stays blank.
  nablaStops: pageCountOf(stopped) === null,
  nablaPageCount: pageCountOf(stopped),
  summationAndIntegralSettle: pageCountOf(settles) !== null,
  summationPageCount: pageCountOf(settles),
});

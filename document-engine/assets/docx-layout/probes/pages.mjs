// What the estimate answers: numbers, or pixel geometry.
// The disposition table's row for `layout-document.ts` rests on this: a created document wants only
// the numbers, which the passes answer before any conversion to pixels.
import { documentOf, paragraph, report } from "./lib.mjs";

// The reference's own entry point takes the formatted body and the context it was formatted in, not
// the document object: handed the document it lays out an empty body and answers the same degenerate
// not-settled result for every document. The packer calls the estimator from `Body.prepForXml`, so a
// wrapper around it captures exactly what the real path hands over — the body before its fields are
// filled — and returns the answer the packer would have got.
const estimateOf = (document) => {
  const body = document.Document.View.Body;
  const pageNumbers = body.pageNumbers;
  let estimate;
  body.pageNumbers = (formatted, context) => {
    estimate = pageNumbers(formatted, context);
    return estimate;
  };
  try {
    body.prepForXml({ file: document, viewWrapper: document.Document, stack: [] });
  } catch (error) {
    return { threw: String(error && error.message) };
  } finally {
    body.pageNumbers = pageNumbers;
  }
  return estimate ?? { threw: "the document was laid out without its estimator" };
};

const one = estimateOf(documentOf([paragraph("Text.")]));
// Two hundred paragraphs take several pages, so the count below changes with the document: an answer
// that stayed at one for both would be one the estimator never worked out.
const many = estimateOf(documentOf(Array.from({ length: 200 }, () => paragraph("Text."))));
report("pages", {
  keys: Object.keys(one).sort(),
  holdsPixelGeometry: Object.hasOwn(one, "pages"),
  onePageCount: one.pageCount ?? null,
  manyPageCount: many.pageCount ?? null,
  manyBookmarkCount: Object.keys(many.bookmarkPageNumbers ?? {}).length,
});

// The columns a created table's cells get, and where the layout stops a table that is too wide.
// The disposition table's rows for `column-widths.ts` and `table-formats.ts` rest on this: the
// reference sizes each column to the content of its cells and widens the columns to the text column
// less half the table's borders, so a table whose cells hold unequal text does not give its columns
// the equal widths its grid states.
import { docx, documentOf, layout, report } from "./lib.mjs";

const table = (rows) =>
  new docx.Table({
    width: { size: 100, type: docx.WidthType.PERCENTAGE },
    rows: rows.map(
      (cells) =>
        new docx.TableRow({
          children: cells.map(
            (text) =>
              new docx.TableCell({
                children: [new docx.Paragraph({ children: [new docx.TextRun({ text })] })],
              }),
          ),
        }),
    ),
  });

/** The height of each row of the table the layout places, and where the layout stopped. */
const rowHeights = (rows) => {
  const { pages, stoppedAt } = layout.layoutDocument(documentOf([table(rows)]));
  return {
    heights: pages[0].body
      .filter((block) => block.type === "table")
      .map((block) => block.rows.map((row) => row.height)),
    stoppedAt: stoppedAt ?? null,
  };
};

// Three hundred points of text: one line in the three hundred and eighty-eight points the column
// gets when the table's cells hold unequal text, and two in the two hundred and twenty-five the grid
// states — so the same text beside itself, which fits its columns evenly, takes two lines.
const long =
  "lorem ipsum dolor sit amet consectetur adipiscing elit sed do eiusmod tempor incididunt ut labore";

report("table", {
  unevenCells: rowHeights([[long, "1"]]),
  evenCells: rowHeights([[long, long]]),
  // The reference lays forty-one columns out and stops at forty-two, where the cells' margins alone
  // are wider than the text column.
  fortyOneColumns: rowHeights([Array.from({ length: 41 }, () => "x")]),
  fortyTwoColumns: rowHeights([Array.from({ length: 42 }, () => "x")]),
});

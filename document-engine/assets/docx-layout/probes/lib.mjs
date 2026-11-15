// The helpers every probe shares: the pinned package, a document built from a create content list,
// and the layout run over it. Run a probe as `bun document-engine/assets/docx-layout/probes/<name>.mjs`;
// each prints one JSON line and exits 1 when the package cannot be found.
import { createRequire } from "node:module";

const require = createRequire(import.meta.url);
const candidates = [
  process.env.PARITY_DOCX,
  `${process.env.HOME}/.bun/install/cache/docx@9.12.0@@@1`,
  new URL("../../../../node_modules/docx", import.meta.url).pathname,
].filter(Boolean);

export const packageDir = candidates.find((path) => {
  try {
    require.resolve(`${path}/package.json`);
    return true;
  } catch {
    return false;
  }
});
if (!packageDir) {
  console.error("docx@9.12.0 is not installed: set PARITY_DOCX to the package directory");
  process.exit(1);
}

export const docx = await import(`${packageDir}/dist/index.mjs`);
export const layout = await import(`${packageDir}/dist/layout.mjs`);

/** A document made of one content list, as the kit's create path makes one. */
export const documentOf = (content, extra = {}) =>
  new docx.Document({
    numbering: {
      config: [
        {
          reference: "document-kit-ordered",
          levels: [{ level: 0, format: docx.NumberFormat.DECIMAL, text: "%1.", alignment: docx.AlignmentType.START }],
        },
      ],
    },
    sections: [{ children: content, ...extra }],
    pageNumbers: layout.estimatePageNumbers,
  });

export const paragraph = (text) => new docx.Paragraph({ children: [new docx.TextRun(text)] });

export const heading = (text, level = 1) =>
  new docx.Paragraph({ text, heading: [docx.HeadingLevel.HEADING_1, docx.HeadingLevel.HEADING_2, docx.HeadingLevel.HEADING_3][level - 1] });

/** A report: the probe's name and what it observed. */
export const report = (name, observed) => {
  console.log(JSON.stringify({ probe: name, ...observed }));
};

/** A package's parts, read from the archive a pack produced: a minimal central-directory walk. */
export const readParts = async (document) => {
  const { inflateRawSync } = await import("node:zlib");
  const buffer = await docx.Packer.toBuffer(document);
  const parts = {};
  // The end-of-central-directory record is the last 22 bytes, or 22 plus a comment.
  let end = buffer.length - 22;
  while (end > 0 && buffer.readUInt32LE(end) !== 0x06054b50) end--;
  const count = buffer.readUInt16LE(end + 10);
  let at = buffer.readUInt32LE(end + 16);
  for (let index = 0; index < count; index++) {
    const method = buffer.readUInt16LE(at + 10);
    const compressed = buffer.readUInt32LE(at + 20);
    const nameLength = buffer.readUInt16LE(at + 28);
    const extraLength = buffer.readUInt16LE(at + 30);
    const commentLength = buffer.readUInt16LE(at + 32);
    const local = buffer.readUInt32LE(at + 42);
    const name = buffer.subarray(at + 46, at + 46 + nameLength).toString("utf8");
    if (!name.endsWith("/")) {
      const start = local + 30 + buffer.readUInt16LE(local + 26) + buffer.readUInt16LE(local + 28);
      const data = buffer.subarray(start, start + compressed);
      parts[name] = method === 0 ? data : inflateRawSync(data);
    }
    at += 46 + nameLength + extraLength + commentLength;
  }
  return parts;
};

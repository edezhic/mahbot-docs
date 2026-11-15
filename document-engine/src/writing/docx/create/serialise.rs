//! The elements a created body is made of: a heading, a list item, a table, a picture, a link and a
//! page break, each serialised as the pinned writer library writes it.
//!
//! These are the library's own shapes rather than a transcription of what they should look like: a
//! created table states its width as a percentage and its own six border sides, its header row's runs
//! are bold and its body rows' state `w:val="false"`; a picture is a `wp:inline` drawing whose
//! namespaces it declares itself, so it does not rely on what the part's root carries; a link's run
//! is the reference's own colour and underline. The canonical model compares these as written, which
//! is why they are reproduced element for element.

use crate::writing::image::{PX_TO_EMU, Picture, Pixels};
use crate::writing::{js_number, js_round};

use super::super::blocks::CREATED_PAGE;
use super::xml::{Element, Node};

/// The twips a created page's text column is wide: the page's own width less its two margins. The
/// table's grid and the table of contents' tab stop are both stated against it.
pub(super) const COLUMN_TWIPS: f64 = CREATED_PAGE.width - CREATED_PAGE.left - CREATED_PAGE.right;

/// The spacing a created picture's paragraph states after it, in twips: the kit's own
/// `IMAGE_PARAGRAPH_SPACING`, which the box a picture is fitted into also subtracts from the column.
pub(super) const IMAGE_PARAGRAPH_SPACING: f64 = 200.0;

/// One row of a table, and whether the reference wrote it as the heading.
pub(super) struct Row {
    pub(super) cells: Vec<String>,
    pub(super) header: bool,
}

/// A heading's paragraph: its style, and one run of its text. An empty heading's text is no run at
/// all, which is how the library writes a paragraph given only `text: ""`.
pub(super) fn heading(text: &str, level: u64) -> Node {
    let mut paragraph = Element::new("w:p").child(
        Element::new("w:pPr")
            .child(Element::new("w:pStyle").attr("w:val", format!("Heading{level}"))),
    );
    if !text.is_empty() {
        paragraph = paragraph.child(Element::run(text));
    }
    paragraph.into()
}

/// A bullet's or a numbered item's paragraph: the list style and the numbering reference the created
/// numbering part states.
pub(super) fn list_item(text: &str, ordered: bool) -> Node {
    let mut paragraph = Element::new("w:p").child(
        Element::new("w:pPr")
            .child(Element::new("w:pStyle").attr("w:val", "ListParagraph"))
            .child(
                Element::new("w:numPr")
                    .child(Element::new("w:ilvl").attr("w:val", "0"))
                    .child(Element::new("w:numId").attr("w:val", if ordered { "2" } else { "1" })),
            ),
    );
    if !text.is_empty() {
        paragraph = paragraph.child(Element::run(text));
    }
    paragraph.into()
}

/// A created table: its width as a percentage, its own border set, the grid its columns share, and
/// one row per row of cells. A body cell states its own non-bold formatting, which is what the
/// library writes for a cell given `bold: false`.
pub(super) fn table(rows: &[Row], columns: usize) -> Node {
    let mut borders = Element::new("w:tblBorders");
    for side in [
        "w:top",
        "w:left",
        "w:bottom",
        "w:right",
        "w:insideH",
        "w:insideV",
    ] {
        borders = borders.child(
            Element::new(side)
                .attr("w:val", "single")
                .attr("w:color", "auto")
                .attr("w:sz", "4"),
        );
    }
    let mut grid = Element::new("w:tblGrid");
    if columns > 0 {
        // Every column is as wide as the others, each rounded on its own — so a three-column grid
        // sums to one twip more than the column it divides.
        #[expect(
            clippy::cast_precision_loss,
            reason = "a column count is a small whole number, exactly representable"
        )]
        let width = js_round(COLUMN_TWIPS / columns as f64);
        for _ in 0..columns {
            grid = grid.child(Element::new("w:gridCol").attr("w:w", js_number(width)));
        }
    }
    let mut table = Element::new("w:tbl")
        .child(
            Element::new("w:tblPr")
                .child(
                    Element::new("w:tblW")
                        .attr("w:type", "pct")
                        .attr("w:w", "5000"),
                )
                .child(borders),
        )
        .child(grid);
    for row in rows {
        let mut tr = Element::new("w:tr");
        for cell in &row.cells {
            let mut run = Element::new("w:r").child(if row.header {
                Element::new("w:rPr")
                    .child(Element::new("w:b"))
                    .child(Element::new("w:bCs"))
            } else {
                Element::new("w:rPr")
                    .child(Element::new("w:b").attr("w:val", "false"))
                    .child(Element::new("w:bCs").attr("w:val", "false"))
            });
            run = run.child(Element::text("w:t", cell.as_str()).attr("xml:space", "preserve"));
            tr = tr.child(Element::new("w:tc").child(Element::new("w:p").child(run)));
        }
        table = table.child(tr);
    }
    table.into()
}

/// A picture's paragraph: the drawing, and the spacing its paragraph states after it.
#[expect(
    clippy::too_many_lines,
    reason = "the library's own long function: one drawing, element for element"
)]
pub(super) fn image(size: Pixels, rel_id: &str, id: &str) -> Node {
    let cx = js_number(js_round(size.width * PX_TO_EMU));
    let cy = js_number(js_round(size.height * PX_TO_EMU));
    let drawing = Element::new("w:drawing").child(
        Element::new("wp:inline")
            .attr("distT", "0")
            .attr("distB", "0")
            .attr("distL", "0")
            .attr("distR", "0")
            .child(
                Element::new("wp:extent")
                    .attr("cx", cx.clone())
                    .attr("cy", cy.clone()),
            )
            .child(
                Element::new("wp:effectExtent")
                    .attr("t", "0")
                    .attr("r", "0")
                    .attr("b", "0")
                    .attr("l", "0"),
            )
            .child(
                Element::new("wp:docPr")
                    .attr("id", id)
                    .attr("name", "")
                    .attr("descr", "")
                    .attr("title", ""),
            )
            .child(
                Element::new("wp:cNvGraphicFramePr").child(
                    Element::new("a:graphicFrameLocks")
                        .attr(
                            "xmlns:a",
                            "http://schemas.openxmlformats.org/drawingml/2006/main",
                        )
                        .attr("noChangeAspect", "1"),
                ),
            )
            .child(
                Element::new("a:graphic")
                    .attr(
                        "xmlns:a",
                        "http://schemas.openxmlformats.org/drawingml/2006/main",
                    )
                    .child(
                        Element::new("a:graphicData")
                            .attr(
                                "uri",
                                "http://schemas.openxmlformats.org/drawingml/2006/picture",
                            )
                            .child(
                                Element::new("pic:pic")
                                    .attr(
                                        "xmlns:pic",
                                        "http://schemas.openxmlformats.org/drawingml/2006/picture",
                                    )
                                    .child(
                                        Element::new("pic:nvPicPr")
                                            .child(
                                                Element::new("pic:cNvPr")
                                                    .attr("id", "0")
                                                    .attr("name", "")
                                                    .attr("descr", ""),
                                            )
                                            .child(
                                                Element::new("pic:cNvPicPr").child(
                                                    Element::new("a:picLocks")
                                                        .attr("noChangeAspect", "1")
                                                        .attr("noChangeArrowheads", "1"),
                                                ),
                                            ),
                                    )
                                    .child(
                                        Element::new("pic:blipFill")
                                            .child(
                                                Element::new("a:blip")
                                                    .attr("r:embed", rel_id)
                                                    .attr("cstate", "none"),
                                            )
                                            .child(Element::new("a:srcRect"))
                                            .child(
                                                Element::new("a:stretch")
                                                    .child(Element::new("a:fillRect")),
                                            ),
                                    )
                                    .child(
                                        Element::new("pic:spPr")
                                            .attr("bwMode", "auto")
                                            .child(
                                                Element::new("a:xfrm")
                                                    .child(
                                                        Element::new("a:off")
                                                            .attr("x", "0")
                                                            .attr("y", "0"),
                                                    )
                                                    .child(
                                                        Element::new("a:ext")
                                                            .attr("cx", cx)
                                                            .attr("cy", cy),
                                                    ),
                                            )
                                            .child(
                                                Element::new("a:prstGeom")
                                                    .attr("prst", "rect")
                                                    .child(Element::new("a:avLst")),
                                            ),
                                    ),
                            ),
                    ),
            ),
    );
    Element::new("w:p")
        .child(
            Element::new("w:pPr").child(
                Element::new("w:spacing").attr("w:after", js_number(IMAGE_PARAGRAPH_SPACING)),
            ),
        )
        .child(Element::new("w:r").child(drawing))
        .into()
}

/// A link's paragraph: the hyperlink with its external target, and the run the reference styles.
pub(super) fn link(text: &str, rel_id: &str, anchor: Option<&str>) -> Node {
    let mut hyperlink = Element::new("w:hyperlink").attr("w:history", "1");
    if let Some(anchor) = anchor {
        hyperlink = hyperlink.attr("w:anchor", anchor);
    }
    let hyperlink = hyperlink.attr("r:id", rel_id).child(
        Element::new("w:r")
            .child(
                Element::new("w:rPr")
                    .child(Element::new("w:color").attr("w:val", "0563C1"))
                    .child(Element::new("w:u").attr("w:val", "single")),
            )
            .child(Element::text("w:t", text).attr("xml:space", "preserve")),
    );
    Element::new("w:p").child(hyperlink).into()
}

/// A page break, as the paragraph and run that carry it.
pub(super) fn page_break() -> Node {
    Element::new("w:p")
        .child(Element::new("w:r").child(Element::new("w:br").attr("w:type", "page")))
        .into()
}

/// The media part's own name, derived from the picture's bytes so one picture placed twice is one
/// part. The reference names it with a SHA-1 of the bytes and this crate with a truncated SHA-256, so
/// the name differs; the canonical model replaces a generated media name with one derived from the
/// content, which is why the digest's flavour is not compared.
pub(super) fn media_name(picture: &Picture) -> String {
    use sha2::Digest as _;
    use std::fmt::Write as _;
    let digest = sha2::Sha256::digest(&picture.bytes);
    let mut hex = String::with_capacity(digest.len() * 2);
    for byte in &digest {
        let _ = write!(hex, "{byte:02x}");
    }
    format!("word/media/{}.{}", &hex[..40], extension(picture))
}

/// The media part's extension, which is the picture's kind as the reference spells it.
fn extension(picture: &Picture) -> &'static str {
    match picture.kind {
        "png" => "png",
        "webp" => "webp",
        _ => "jpg",
    }
}

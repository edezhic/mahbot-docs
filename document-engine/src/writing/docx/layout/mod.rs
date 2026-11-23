//! The layout pass a created document's page numbers come from.
//!
//! This module tree is a transliteration of the pinned library's own layout, read side by side with
//! its source: a function here is a function there, a field here is a field there, and a count is
//! cast where the reference multiplies one. A deviation from the reference's shape states its reason
//! in a targeted lint expectation rather than in a blanket allowance.
//!
//! A created document is given an estimate of its own pages only when it carries a table of contents
//! or a `{pages}` token. The pass lays the document out as Word does — measuring text with the widths
//! the fonts Word documents use most have, breaking lines at the column's own width, and paginating
//! between them — and answers the page each bookmark starts on and how many pages the document has.
//! A table's columns are sized as the reference sizes them: each as wide as the content of its cells,
//! then widened or narrowed to the width the table gives itself, less half its borders.
//!
//! The document it lays out is the one this writer produced: the pass reads the formatted body and
//! the file parts back through its own reader, which is what the reference does and what makes its
//! numbers reproducible. It is not a walk over the request's blocks.
//!
//! # What it covers
//!
//! What the pass covers is what [`disposition`] states, not the whole of the library. The crate's own
//! tests close it from the outside: `document-engine/tests/docx_layout.rs` lays out parts from the
//! independent builder in `document-engine/src/test_support/docx_fixture.rs` and the writer's own
//! documents, `document-engine/tests/docx_layout_modules.rs` holds the table to the module list, and
//! the recorded `docx/create` cases in the gate judge the writer against the record.
//!
//! # The outcome no recorded case reaches
//!
//! The failure to settle — numbers that still move after the reference's three passes — has no
//! recorded case. The probes tried were a contents entry whose heading is 40 to 120 characters long
//! over a body of 56 one-line paragraphs, each with and without a `{pages}` footer, and every one
//! settled. The pass's own seam does reach the outcome:
//! `paginate::tests::numbers_still_moving_after_the_last_pass_are_answered_with_none` builds a
//! document whose numbers move between the first two passes and settle by the third, and answers it
//! with none when it is allowed two. No created document is known to be still moving after the
//! reference's three. The throw is forced the same way, through [`estimate`]: parts the reader cannot
//! read answer [`Layout::Failed`], which is the re-write path a crate-side test pins.
//!
//! # The reference this is a port of
//!
//! The reference's layout lives in the pinned writer library's `docx/layout` entry point, which
//! bundles thirty source modules. The disposition table in [`disposition`] states, one row per module
//! the pass reaches, what this port does with it, and names the probe beside the data that the row's
//! exclusion rests on. The library's exact version is named there, and an upgrade of it is a separate
//! deliberate change that re-derives the width data, the layout constants and the module list — not
//! something a dependency bump may do silently.

pub(crate) mod breaks;
pub(crate) mod constants;
/// The disposition table is read by the isolated test, which compiles this crate with the
/// test-support feature; a normal build carries the data only.
#[cfg(any(test, feature = "test-support"))]
pub mod disposition;
pub(crate) mod paginate;
pub(crate) mod read;
pub(crate) mod text;

use std::collections::BTreeMap;

/// What the layout worked out: the numbers a table of contents' page references and a page-count
/// token are written with. Two passes that answer the same value have settled.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Estimate {
    /// Each bookmark's own page number, which is what a page reference writes.
    pub bookmark_numbers: BTreeMap<String, u32>,
    /// How many pages the document has, when the whole of it was laid out.
    pub page_count: Option<u32>,
}

/// What one layout pass answered.
///
/// The three answers are the reference's three outcomes, and they are not interchangeable: a pass
/// that worked the numbers out writes them; one whose numbers never settled leaves every number blank
/// and written clean, so the application shows nothing rather than filling it in; and one that failed
/// outright makes the reference write the document again without any estimate, which leaves the
/// fields dirty — a distinction that exists only for the contents and page-reference fields.
#[derive(Debug)]
pub enum Layout {
    /// The numbers the pass worked out. A pass that stopped at something it cannot lay out still
    /// answers, with the places it found before the stop and no page count.
    Numbers(Estimate),
    /// No number could be worked out: every field is written clean and blank.
    Blank,
    /// The pass failed, so the document is written again without it.
    Failed,
}

/// What a created document's parts say, as the layout's reader sees them: the fixture the layout's
/// own tests are proved against.
#[cfg(any(test, feature = "test-support"))]
#[derive(Debug)]
pub struct Summary {
    pub blocks: usize,
    pub paragraphs: usize,
    pub tables: usize,
    pub footnotes: usize,
    pub bookmarks: Vec<String>,
    pub page_width: f64,
    pub page_height: f64,
}

/// Read `parts` and summarise the model, or `None` when they are not readable XML.
#[cfg(any(test, feature = "test-support"))]
pub fn summary(parts: &[(String, Vec<u8>)]) -> Option<Summary> {
    let content = read::Content::read(parts).ok()?;
    let mut paragraphs = 0;
    let mut tables = 0;
    let mut bookmarks = Vec::new();
    let mut count = |blocks: &[read::Block]| {
        for block in blocks {
            match block {
                read::Block::Paragraph(paragraph) => {
                    paragraphs += 1;
                    if let Some(name) = &paragraph.bookmark {
                        bookmarks.push(name.clone());
                    }
                }
                read::Block::Table(_) => tables += 1,
            }
        }
    };
    count(&content.blocks);
    count(&content.header);
    count(&content.footer);
    Some(Summary {
        blocks: content.blocks.len(),
        paragraphs,
        tables,
        footnotes: content.footnotes.len(),
        bookmarks,
        page_width: content.page.width,
        page_height: content.page.height,
    })
}

/// Lay `parts` out and answer the numbers a created document writes.
pub fn estimate(parts: &[(String, Vec<u8>)]) -> Layout {
    // A part the pass cannot read is the crate's own defect rather than the caller's: the writer
    // built it, and the reference's answer for a layout it cannot carry out is a document written
    // again without one.
    let Ok(content) = read::Content::read(parts) else {
        return Layout::Failed;
    };
    match paginate::passes(&content) {
        Some(estimate) => Layout::Numbers(estimate),
        None => Layout::Blank,
    }
}

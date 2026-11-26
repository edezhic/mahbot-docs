//! The page tree a reading walks for itself: a tree nested thousands of levels deep, a page whose
//! dictionary nests past the structure reader's bound, a page the tree names twice — with and without
//! a raster — a tree that states a hostile page count, `/Kids` entries the file does not resolve or
//! that are not dictionaries at all, and a picture defined on an ancestor node.
//!
//! Two documents this module reads are corpus readings. `inherited-image.pdf` is read the same by
//! both sides. `page-dictionary-inline-beside-a-reference.pdf` is read differently: the product
//! reader's text pass knows only the referenced page and draws it alone, while the engine draws both
//! pages from the tree's own entries, so the corpus holds that one as a declared reading divergence.
//! The other shapes are witnessed here alone, on a thread with the two mebibytes a product conversion
//! runs on: the reference's recursive page-tree walk overflows the stack on the first, which is a
//! reader failure no declaration forgives, and reads the others differently — a twice-named page is
//! delivered twice, a page nested past the bound is parsed, a dangling entry is skipped, and a page
//! it declines is numbered where the engine reports it. The two public page-render answers they
//! decide are witnessed beside them, and each test here is the crate's own, so
//! `cargo test -p document-engine` alone still catches a picture being lost.

use document_engine::reading::{Reading, RenderError, read, render_page};
use document_engine::test_support::{Document, documents};
use document_engine::vocabulary::vocabulary;
use std::path::Path;

/// The stack a product conversion runs on: a blocking thread's default two mebibytes.
const CONVERSION_STACK: usize = 2 * 1024 * 1024;

/// Write `document` into a fresh temporary directory, which the caller keeps alive, and answer the
/// path it was written to.
fn write_document(document: Document) -> (tempfile::TempDir, std::path::PathBuf) {
    let dir = tempfile::TempDir::new().expect("a temporary directory");
    let path = dir.path().join(document.name);
    std::fs::write(&path, (document.build)()).expect("write the input");
    (dir, path)
}

/// Read `document` on a thread with [`CONVERSION_STACK`], from a fresh directory, and answer the
/// reading with the names of the pictures it wrote.
fn read_on_a_conversion_stack(document: Document) -> (Reading, Vec<String>) {
    let (dir, path) = write_document(document);
    let out = dir.path().join("out");
    let name = document.name.to_string();
    let out_dir = out.clone();
    let reading = std::thread::Builder::new()
        .stack_size(CONVERSION_STACK)
        .spawn(move || read(&path, &name, &out_dir))
        .expect("spawn the conversion thread")
        .join()
        .expect("a reading never takes the process down");
    let pictures = picture_names(&out);
    (reading, pictures)
}

/// Every file the reading wrote into `out`, by name, sorted.
fn picture_names(out: &Path) -> Vec<String> {
    let Ok(entries) = std::fs::read_dir(out) else {
        return Vec::new();
    };
    let mut names: Vec<String> = entries
        .filter_map(Result::ok)
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
}

/// The text arm's fields, or a panic naming the arm that was returned instead.
fn text(reading: Reading) -> (String, Vec<String>, Vec<String>, bool) {
    match reading {
        Reading::Text {
            text,
            notes,
            unshown,
            all_page_text_lost,
            ..
        } => (text, notes, unshown, all_page_text_lost),
        other => panic!("expected a reading with text, got {other:?}"),
    }
}

/// The label the reading prints above the text of page `page`.
fn block(page: usize) -> String {
    mark("block_page", &[("n", &page.to_string())])
}

/// The first line of each block of `text`, in the reading's order: a page block's own label, a
/// description's, an annotation's.
fn block_heads(text: &str) -> Vec<String> {
    text.split("\n\n")
        .filter_map(|block| block.lines().next())
        .map(str::to_string)
        .collect()
}

/// The mark `key` spells, with `values` filled in.
fn mark(key: &str, values: &[(&str, &str)]) -> String {
    vocabulary().marks.format(key, values)
}

/// The not-shown line for `count` pages, as the reading prints it.
fn pages_not_shown(count: usize) -> String {
    mark(
        "unshown_line",
        &[
            ("count", &count.to_string()),
            ("what", vocabulary().marks.get("unshown_page")),
        ],
    )
}

#[test]
fn a_page_tree_nested_thousands_of_levels_is_read() {
    let (reading, pictures) = read_on_a_conversion_stack(documents::PAGE_TREE_NESTED_THOUSANDS_PDF);
    let (text, notes, unshown, lost) = text(reading);
    // The page the tree ends at is delivered with its own text under its own label, and the picture
    // a page with that little text is given: the walk costs heap, not the conversion thread's stack.
    assert!(text.contains("Page one text"), "{text}");
    assert_eq!(block_heads(&text), vec![block(1)], "{text}");
    assert_eq!(pictures, vec!["page_1.jpg".to_string()]);
    assert!(notes.is_empty(), "{notes:?}");
    assert!(unshown.is_empty(), "the page was delivered: {unshown:?}");
    assert!(!lost, "the text pass read the page");
}

#[test]
fn a_page_nested_past_the_readers_bound_is_answered_as_pages_that_could_not_be_read() {
    let (reading, pictures) =
        read_on_a_conversion_stack(documents::PAGE_OBJECT_NESTED_PAST_THE_BOUND_PDF);
    assert_eq!(
        reading,
        Reading::Unreadable {
            reason: mark("reason_pages_could_not_be_read", &[]),
        }
    );
    assert!(pictures.is_empty());
}

#[test]
fn a_page_tree_that_names_itself_is_answered_as_pages_that_could_not_be_read() {
    let (reading, pictures) = read_on_a_conversion_stack(documents::PAGE_TREE_NAMES_ITSELF_PDF);
    // The tree names itself: the walk must stop at the cycle rather than descend it forever, and a
    // tree that reaches no page answers as one whose pages could not be read.
    assert_eq!(
        reading,
        Reading::Unreadable {
            reason: mark("reason_pages_could_not_be_read", &[]),
        }
    );
    assert!(pictures.is_empty());
}

#[test]
fn a_page_the_tree_names_twice_is_delivered_once_and_reported() {
    let (reading, pictures) = read_on_a_conversion_stack(documents::PAGE_NAMED_TWICE_PDF);
    let (text, notes, unshown, _lost) = text(reading);
    // Both namings keep the page's text; the structure reader delivered the page once, so the
    // second naming is reported as a page not shown.
    assert_eq!(block_heads(&text), vec![block(1), block(2)], "{text}");
    assert_eq!(unshown, vec![pages_not_shown(1)]);
    assert!(notes.is_empty(), "{notes:?}");
    assert!(pictures.is_empty());
}

#[test]
fn a_declined_naming_that_carries_text_is_reported_not_dropped() {
    let (reading, pictures) =
        read_on_a_conversion_stack(documents::PAGE_NAMED_TWICE_WITH_A_RASTER_PDF);
    let (text, notes, unshown, _lost) = text(reading);
    // Both namings keep the page's text, and that text is too short to stand alone, so the first
    // naming draws the page. The second naming is one the structure reader did not deliver: it is
    // reported as not shown rather than silently stripped of its picture.
    assert_eq!(block_heads(&text), vec![block(1), block(2)], "{text}");
    assert_eq!(pictures, vec!["page_1.jpg".to_string()]);
    assert_eq!(unshown, vec![pages_not_shown(1)]);
    assert!(notes.is_empty(), "{notes:?}");
}

#[test]
fn the_render_entry_counts_a_twice_named_page_once() {
    let (_dir, path) = write_document(documents::PAGE_NAMED_TWICE_PDF);
    // The reference resolves the page twice and renders the second naming; the reader resolves it
    // once, so the second page number is past the end and the answer carries the count it has.
    assert_eq!(
        render_page(&path, 2, 200, 2.0),
        Err(RenderError::NoSuchPage { pages: 1 })
    );
}

#[test]
fn the_render_entry_has_no_page_the_structure_reader_declined() {
    let (_dir, path) = write_document(documents::PAGE_OBJECT_NESTED_PAST_THE_BOUND_PDF);
    // The reference parses the page and renders it; the reader declines it, so its page list is
    // empty and there is no page one to answer with.
    assert_eq!(
        render_page(&path, 1, 200, 2.0),
        Err(RenderError::NoSuchPage { pages: 0 })
    );
}

#[test]
fn a_kid_the_file_does_not_hold_is_reported_as_a_page_not_shown() {
    let (reading, pictures) =
        read_on_a_conversion_stack(documents::PAGE_TREE_WITH_A_DANGLING_KID_PDF);
    let (text, notes, unshown, _lost) = text(reading);
    // The page the file holds is read; the kid it names and does not hold is one nothing was
    // delivered for, so it is reported rather than passed over.
    assert_eq!(block_heads(&text), vec![block(1)], "{text}");
    assert_eq!(unshown, vec![pages_not_shown(1)]);
    assert!(notes.is_empty(), "{notes:?}");
    assert_eq!(pictures, vec!["page_1.jpg".to_string()]);
}

#[test]
fn a_kid_that_is_not_a_dictionary_is_not_a_page() {
    let (reading, pictures) =
        read_on_a_conversion_stack(documents::PAGE_TREE_WITH_A_KID_THAT_IS_NOT_A_DICTIONARY_PDF);
    let (text, notes, unshown, _lost) = text(reading);
    // The kids resolve to numbers — one by reference and named twice, one stated inline — which no
    // reader counts as a page: nothing is reported as not shown, and the page the file holds is
    // read.
    assert_eq!(block_heads(&text), vec![block(1)], "{text}");
    assert_eq!(pictures, vec!["page_1.jpg".to_string()]);
    assert!(unshown.is_empty(), "{unshown:?}");
    assert!(notes.is_empty(), "{notes:?}");
}

#[test]
fn two_pages_stated_inline_under_one_node_are_both_delivered() {
    let (reading, pictures) =
        read_on_a_conversion_stack(documents::PAGE_TREE_WITH_TWO_INLINE_PAGES_PDF);
    let (_text, notes, unshown, _lost) = text(reading);
    // Both page dictionaries carry the `/Pages` node's object number, so they are claimed in order:
    // each is delivered and drawn, and neither is reported as a page the reader did not deliver.
    // The text pass names no inline page, so it reads neither — the loss is reported, not the pages.
    assert_eq!(
        pictures,
        vec!["page_1.jpg".to_string(), "page_2.jpg".to_string()]
    );
    assert!(unshown.is_empty(), "{unshown:?}");
    assert_eq!(notes.len(), 1, "the lost text is stated: {notes:?}");
}

#[test]
fn a_picture_defined_on_an_ancestor_is_still_delivered() {
    let (reading, pictures) = read_on_a_conversion_stack(documents::INHERITED_IMAGE_PDF);
    let (text, notes, unshown, _lost) = text(reading);
    // The page's own resources name only its font; the picture is on the node above it.
    assert_eq!(pictures, vec!["page_1_img_0.jpg".to_string()]);
    assert_eq!(block_heads(&text), vec![block(1)], "{text}");
    assert!(notes.is_empty(), "{notes:?}");
    assert!(unshown.is_empty(), "{unshown:?}");
}

#[test]
fn every_declined_page_is_reported_beyond_the_pages_delivered() {
    let (reading, pictures) =
        read_on_a_conversion_stack(documents::PAGE_TREE_DECLINING_MORE_PAGES_THAN_IT_DELIVERS_PDF);
    let (text, notes, unshown, _lost) = text(reading);
    // One page is delivered and two are declined: both are reported, not one.
    assert_eq!(block_heads(&text), vec![block(1)], "{text}");
    assert_eq!(unshown, vec![pages_not_shown(2)]);
    assert!(notes.is_empty(), "{notes:?}");
    assert!(pictures.is_empty(), "{pictures:?}");
}

#[test]
fn a_hostile_count_cannot_inflate_the_not_shown_report() {
    let (reading, pictures) =
        read_on_a_conversion_stack(documents::PAGE_TREE_WITH_A_HOSTILE_COUNT_PDF);
    let (text, notes, unshown, lost) = text(reading);
    // The tree names one node a hundred times and states a million pages under it. One budget for
    // the whole walk means the report cannot claim more pages than the two the file delivers; the
    // rest of the answer is the one a two-page file with a repeated node gives: the text pass
    // numbers its four pages, the first two are drawn from the pages they are, and the two the
    // repeated naming stands for are reported rather than drawn.
    assert_eq!(
        block_heads(&text),
        vec![block(1), block(2), block(3), block(4)],
        "{text}"
    );
    assert_eq!(
        pictures,
        vec!["page_1.jpg".to_string(), "page_2.jpg".to_string()]
    );
    assert_eq!(unshown, vec![pages_not_shown(2)]);
    assert_eq!(notes, vec![mark("note_pages_unaligned", &[])]);
    assert!(!lost, "the text pass read every page");
}

#[test]
fn a_picture_is_not_paired_by_position_when_the_page_lists_cannot_be_aligned() {
    let (reading, pictures) =
        read_on_a_conversion_stack(documents::PAGE_DICTIONARY_INLINE_BESIDE_A_REFERENCE_PDF);
    let (text, notes, unshown, _lost) = text(reading);
    // The text pass knows only the referenced page, so it prints that page's text at position one,
    // where the tree holds the inline page. Both pages are drawn from their own entry — the inline
    // page at page 1, the referenced one at page 2 — and the note says the text's numbering is not
    // the tree's.
    assert_eq!(
        pictures,
        vec!["page_1.jpg".to_string(), "page_2.jpg".to_string()]
    );
    assert_eq!(block_heads(&text), vec![block(1)], "{text}");
    assert!(unshown.is_empty(), "{unshown:?}");
    assert_eq!(notes, vec![mark("note_pages_unaligned", &[])]);
}

#[test]
fn a_declined_page_between_two_delivered_ones_is_stated() {
    let (reading, pictures) =
        read_on_a_conversion_stack(documents::PAGE_TREE_WITH_A_DECLINED_PAGE_BETWEEN_PDF);
    let (text, notes, unshown, _lost) = text(reading);
    // The text pass numbers the last page one lower than the tree does, so the two lists name
    // different pages at position two: the reading says so, and the last page's picture still
    // comes from the last page.
    assert_eq!(block_heads(&text), vec![block(1), block(2)], "{text}");
    assert_eq!(unshown, vec![pages_not_shown(1)]);
    assert_eq!(notes, vec![mark("note_pages_unaligned", &[])]);
    assert_eq!(
        pictures,
        vec!["page_1.jpg".to_string(), "page_3.jpg".to_string()]
    );
}

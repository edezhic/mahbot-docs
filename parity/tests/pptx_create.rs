//! What a created deck's package says about itself, beyond what the recorded model compares.
//!
//! The canonical model normalises away the two moments a deck is dated with, drops the revision,
//! and excludes `docProps/app.xml` entirely, so a created deck whose dates are a local time wearing
//! a `Z`, whose revision is any number at all, or whose application properties state a different
//! slide count, notes count, title list or format string compares equal to the reference under that
//! model. These tests compare those things instead, outside the recorded baseline, by driving the
//! reference writer and the crate's own writer over the same request and reading both produced
//! packages.
//!
//! Three claims:
//!
//! - [`the_core_properties_are_the_references_strings_with_this_runs_utc_dates`]: the library's own
//!   fixed strings, its revision of 1, and two stamps that are in the shape the format states, mean
//!   UTC, and sit within a stated tolerance of this run's clock.
//! - [`the_application_properties_agree_by_content_across_slides_notes_titles_and_sizes`]: the part
//!   compared by content, because it is content and not bytes that must agree — the two sides spell
//!   the same elements with different whitespace between them.
//! - [`the_archive_holds_exactly_the_references_entries_and_no_folders`]: the archive's entry set,
//!   so a part either side omits or adds is visible, and no folder entry is written.
//!
//! None of the three skips. [`ReferenceWriter::new`] fails when the runtime the kit needs (`bun` on
//! `PATH`, or `PARITY_BUN`) is absent, and that failure is left to panic: a comparison that did not
//! run is not a comparison that passed.

use document_engine::operation::{Destination, Request, execute};
use document_parity::canonical::{canonicalize, compare};
use document_parity::corpus::build;
use document_parity::engine::{Family, Writer};
use document_parity::pptx_capture;
use document_parity::reference::{CrateWriter, ReferenceWriter};
use regex::Regex;
use serde_json::{Value, json};
use std::collections::BTreeSet;
use std::path::Path;
use std::sync::LazyLock;

/// How far a produced deck's stamps may sit from this run's own clock, in seconds. Both engines
/// stamp "now" as they write, one after the other, so equality is the wrong claim; five minutes
/// leaves room for a loaded machine and still fails a stamp read from a local clock, which is out by
/// the machine's zone offset — hours, wherever the zone is more than five minutes from UTC. That is
/// the mistake this test exists for; a machine whose zone is under five minutes from Greenwich
/// cannot show it, and the crate's `now_w3cdtf` is then the only statement that its `Z` means UTC.
const TOLERANCE_SECS: i64 = 300;

/// The library's own strings, which no request changes.
const TITLE: &str = "PptxGenJS Presentation";
const SUBJECT: &str = "PptxGenJS Presentation";
const CREATOR: &str = "PptxGenJS";
const LAST_MODIFIED_BY: &str = "PptxGenJS";
const REVISION: &str = "1";

// ── the core properties ──────────────────────────────────────────────────────────────────────

#[test]
fn the_core_properties_are_the_references_strings_with_this_runs_utc_dates() {
    let scratch = tempfile::tempdir().expect("a temporary directory");
    let reference =
        ReferenceWriter::new(&scratch.path().join("engine")).expect("the reference writer");
    let crate_writer = CrateWriter;

    for (name, content) in core_decks() {
        let theirs = produced(
            &reference,
            &scratch.path().join("reference"),
            name,
            &content,
            None,
        );
        let ours = produced(
            &crate_writer,
            &scratch.path().join("crate"),
            name,
            &content,
            None,
        );
        for (side, deck) in [("the reference", &theirs), ("the crate", &ours)] {
            let core = content_of(&part(deck, "docProps/core.xml"));
            for (element, expected) in [
                ("dc:title", TITLE),
                ("dc:subject", SUBJECT),
                ("dc:creator", CREATOR),
                ("cp:lastModifiedBy", LAST_MODIFIED_BY),
                ("cp:revision", REVISION),
            ] {
                assert_eq!(
                    element_text(&core, element).as_deref(),
                    Some(expected),
                    "{name}: {side}'s {element}"
                );
            }
            let now = now_seconds();
            for element in ["dcterms:created", "dcterms:modified"] {
                let stamp = element_text(&core, element)
                    .unwrap_or_else(|| panic!("{name}: {side} states no {element}"));
                let seconds = utc_seconds(&stamp);
                assert!(
                    (seconds - now).abs() <= TOLERANCE_SECS,
                    "{name}: {side}'s {element} is {stamp}, which is not within {TOLERANCE_SECS}s \
                     of this run: a local time wearing a `Z` is out by the machine's zone offset"
                );
                // The attribute is what makes the text a W3CDTF stamp rather than a string of
                // digits: a reading that ignored it would be reading an untyped element.
                assert_eq!(
                    attribute_of(&core, element, "xsi:type").as_deref(),
                    Some("dcterms:W3CDTF"),
                    "{name}: {side}'s {element} states no W3CDTF type"
                );
            }
        }
    }
}

/// The decks the core properties are compared over: one slide, and several slides whose titles and
/// speaker notes are the only thing the part could have taken a value from.
fn core_decks() -> Vec<(&'static str, Value)> {
    vec![
        (
            "one-slide",
            json!([
                { "type": "heading", "level": 1, "text": "A deck of one slide" },
                { "type": "paragraph", "text": "The only body paragraph." },
            ]),
        ),
        (
            "several-slides",
            json!([
                { "type": "heading", "level": 1, "text": "The opening slide" },
                { "type": "paragraph", "text": "Its body." },
                { "type": "notes", "text": "A speaker note." },
                { "type": "heading", "level": 1, "text": "The middle slide" },
                { "type": "list", "items": ["One", "Two"] },
                { "type": "notes", "text": "Another speaker note." },
                { "type": "heading", "level": 1, "text": "The closing slide" },
            ]),
        ),
    ]
}

// ── the application properties ──────────────────────────────────────────────────────────────

#[test]
fn the_application_properties_agree_by_content_across_slides_notes_titles_and_sizes() {
    let scratch = tempfile::tempdir().expect("a temporary directory");
    let reference =
        ReferenceWriter::new(&scratch.path().join("engine")).expect("the reference writer");
    let crate_writer = CrateWriter;

    // The axes the part states something about — how many slides there are, how many of them carry
    // notes, what the slides are titled — and the one it states nothing about, the deck's own size.
    // The portrait deck is the same content as the widescreen one, so the size is the only
    // difference between them.
    let decks = [
        (
            "one-slide",
            None,
            json!([
                { "type": "heading", "level": 1, "text": "A deck of one slide" },
                { "type": "paragraph", "text": "The only body paragraph." },
            ]),
            1,
            0,
            vec!["A deck of one slide"],
        ),
        (
            "several-slides",
            Some("widescreen"),
            json!([
                { "type": "heading", "level": 1, "text": "The opening slide" },
                { "type": "paragraph", "text": "Its body." },
                { "type": "notes", "text": "A speaker note." },
                { "type": "heading", "level": 1, "text": "The middle slide" },
                { "type": "list", "items": ["One", "Two"] },
                { "type": "notes", "text": "Another speaker note." },
                { "type": "heading", "level": 1, "text": "The closing slide" },
            ]),
            3,
            2,
            vec!["The opening slide", "The middle slide", "The closing slide"],
        ),
        (
            "portrait",
            Some("portrait"),
            json!([
                { "type": "heading", "level": 1, "text": "The opening slide" },
                { "type": "paragraph", "text": "Its body." },
                { "type": "notes", "text": "A speaker note." },
                { "type": "heading", "level": 1, "text": "The middle slide" },
                { "type": "list", "items": ["One", "Two"] },
                { "type": "notes", "text": "Another speaker note." },
                { "type": "heading", "level": 1, "text": "The closing slide" },
            ]),
            3,
            2,
            vec!["The opening slide", "The middle slide", "The closing slide"],
        ),
    ];

    for (name, size, content, slides, notes, titles) in decks {
        let theirs = produced(
            &reference,
            &scratch.path().join("reference"),
            name,
            &content,
            size,
        );
        let ours = produced(
            &crate_writer,
            &scratch.path().join("crate"),
            name,
            &content,
            size,
        );
        let theirs = content_of(&part(&theirs, "docProps/app.xml"));
        let ours = content_of(&part(&ours, "docProps/app.xml"));

        assert_eq!(
            ours, theirs,
            "{name}: the crate's application properties are not the reference's"
        );
        assert_eq!(
            element_text(&ours, "Slides"),
            Some(slides.to_string()),
            "{name}: the part states the deck's own slide count"
        );
        assert_eq!(
            element_text(&ours, "Notes"),
            Some(notes.to_string()),
            "{name}: the part states the deck's own notes count"
        );
        let stated = part_titles(&ours);
        let stated: Vec<&str> = stated
            .iter()
            .map(String::as_str)
            .filter(|title| titles.contains(title))
            .collect();
        assert_eq!(
            stated, titles,
            "{name}: the part states each slide's own title"
        );
        // The part states the library's format string whatever the deck's size, so a portrait deck
        // says 16:9 too. Both sides state it, which the comparison above already holds; this pins
        // the value, so a change to the string is visible as itself rather than as a diff.
        assert_eq!(
            element_text(&ours, "PresentationFormat").as_deref(),
            Some("On-screen Show (16:9)"),
            "{name}: the crate's format string"
        );
        assert_eq!(
            element_text(&theirs, "PresentationFormat").as_deref(),
            Some("On-screen Show (16:9)"),
            "{name}: the reference's format string"
        );
    }
}

// ── the archive's entries ───────────────────────────────────────────────────────────────────

#[test]
fn the_archive_holds_exactly_the_references_entries_and_no_folders() {
    let scratch = tempfile::tempdir().expect("a temporary directory");
    let reference =
        ReferenceWriter::new(&scratch.path().join("engine")).expect("the reference writer");
    let crate_writer = CrateWriter;
    let content = json!([
        { "type": "heading", "level": 1, "text": "The opening slide" },
        { "type": "paragraph", "text": "Its body." },
        { "type": "notes", "text": "A speaker note." },
        { "type": "heading", "level": 1, "text": "The closing slide" },
        { "type": "table", "headers": ["Left"], "rows": [["Right"]] },
    ]);

    let theirs = produced(
        &reference,
        &scratch.path().join("reference"),
        "entries",
        &content,
        Some("standard"),
    );
    let ours = produced(
        &crate_writer,
        &scratch.path().join("crate"),
        "entries",
        &content,
        Some("standard"),
    );
    let ours_entries = entries(&ours);
    let theirs_entries = entries(&theirs);

    assert_eq!(
        ours_entries, theirs_entries,
        "the crate writes a part the reference does not, or leaves one out"
    );
    assert!(
        ours_entries.contains("docProps/core.xml"),
        "the archive holds the core properties part"
    );
    assert!(
        ours_entries.iter().all(|name| !name.ends_with('/')),
        "the archive states a folder entry: {ours_entries:?}"
    );
}

// ── driving both writers ────────────────────────────────────────────────────────────────────

/// Run one create request through one writer and read back the deck it wrote.
fn produced(
    writer: &dyn Writer,
    root: &Path,
    name: &str,
    content: &Value,
    size: Option<&str>,
) -> Vec<u8> {
    let directory = root.join(name);
    std::fs::create_dir_all(&directory)
        .unwrap_or_else(|e| panic!("cannot create {}: {e}", directory.display()));
    let scratch = directory.join("scratch");
    std::fs::create_dir_all(&scratch)
        .unwrap_or_else(|e| panic!("cannot create {}: {e}", scratch.display()));
    let output = directory.join(format!("{name}.pptx"));
    let mut request = json!({
        "op": "create",
        "format": "pptx",
        "content": content.clone(),
        "output": output.to_string_lossy(),
    });
    if let Some(size) = size {
        request["size"] = json!(size);
    }
    let outcome = writer
        .run(&request, &scratch)
        .unwrap_or_else(|error| panic!("{name}: the writer did not run the request: {error}"));
    assert!(
        outcome.failure.is_none(),
        "{name}: the writer refused the request: {:?}",
        outcome.failure
    );
    assert_eq!(
        outcome.outputs,
        vec![output.clone()],
        "{name}: the writer named the file it wrote"
    );
    std::fs::read(&output)
        .unwrap_or_else(|error| panic!("{name}: cannot read {}: {error}", output.display()))
}

// ── reading a part ──────────────────────────────────────────────────────────────────────────

/// Every entry the archive states, folder entries included, by name.
fn entries(deck: &[u8]) -> BTreeSet<String> {
    let mut archive = zip::ZipArchive::new(std::io::Cursor::new(deck))
        .expect("the deck is a readable ZIP archive");
    let mut names = BTreeSet::new();
    for index in 0..archive.len() {
        let entry = archive
            .by_index(index)
            .unwrap_or_else(|e| panic!("the deck's entry {index} is unreadable: {e}"));
        names.insert(entry.name().to_string());
    }
    names
}

/// One part's bytes, by the archive's own name for it.
fn part(deck: &[u8], name: &str) -> Vec<u8> {
    pptx_capture::read_parts(deck)
        .unwrap_or_else(|error| panic!("the deck is not a readable archive: {error:#}"))
        .remove(name)
        .unwrap_or_else(|| panic!("the deck holds no part named {name}"))
}

/// A part's text with the whitespace between two elements removed: the two sides state the same
/// elements and differ only in how they indent them.
fn content_of(part: &[u8]) -> String {
    static BETWEEN: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(">\\s+<").expect("a pattern must build a regular expression"));
    BETWEEN
        .replace_all(&String::from_utf8_lossy(part), "><")
        .into_owned()
}

/// The text of every `<name>` element of `xml`, in document order, whether it states attributes or
/// not.
fn element_texts(xml: &str, name: &str) -> Vec<String> {
    Regex::new(&format!("(?s)<{name}(?:\\s[^>]*)?>(.*?)</{name}>"))
        .expect("a pattern must build a regular expression")
        .captures_iter(xml)
        .filter_map(|found| found.get(1).map(|text| text.as_str().to_string()))
        .collect()
}

/// The text of the first `<name>` element of `xml`.
fn element_text(xml: &str, name: &str) -> Option<String> {
    element_texts(xml, name).into_iter().next()
}

/// The slide titles `TitlesOfParts` states.
fn part_titles(xml: &str) -> Vec<String> {
    static TITLES: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new("(?s)<TitlesOfParts>(.*?)</TitlesOfParts>")
            .expect("a pattern must build a regular expression")
    });
    TITLES
        .captures(xml)
        .and_then(|found| found.get(1))
        .map_or_else(Vec::new, |inner| element_texts(inner.as_str(), "vt:lpstr"))
}

/// The value `xml`'s first `<element>` states for the attribute `name`.
fn attribute_of(xml: &str, element: &str, name: &str) -> Option<String> {
    let start = xml.find(&format!("<{element}"))?;
    let rest = xml.get(start..)?;
    let tag = rest.get(..rest.find('>')?)?;
    let needle = format!("{name}=\"");
    let at = tag.find(&needle)? + needle.len();
    let value = tag.get(at..)?;
    value.get(..value.find('"')?).map(str::to_string)
}

// ── the stamps ──────────────────────────────────────────────────────────────────────────────

/// This run's clock, in seconds since the epoch.
fn now_seconds() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("the clock is after the epoch")
        .as_secs()
        .try_into()
        .expect("the moment fits an i64")
}

/// The seconds since the epoch a `YYYY-MM-DDTHH:MM:SSZ` stamp states. A stamp of any other shape
/// panics: the shape is part of what the two sides must state.
fn utc_seconds(stamp: &str) -> i64 {
    assert_eq!(
        stamp.len(),
        20,
        "{stamp:?} is not spelled YYYY-MM-DDTHH:MM:SSZ"
    );
    let number = |from: usize, to: usize| -> i64 {
        let digits = &stamp[from..to];
        assert!(
            digits.bytes().all(|byte| byte.is_ascii_digit()),
            "{stamp:?} is not spelled YYYY-MM-DDTHH:MM:SSZ"
        );
        digits.parse().expect("a run of digits")
    };
    for (at, separator) in [(4, '-'), (7, '-'), (10, 'T'), (13, ':'), (16, ':')] {
        assert_eq!(
            stamp.as_bytes()[at],
            separator as u8,
            "{stamp:?} is not spelled YYYY-MM-DDTHH:MM:SSZ"
        );
    }
    assert_eq!(&stamp[19..], "Z", "{stamp:?} does not state UTC");
    let (year, month, day) = (number(0, 4), number(5, 7), number(8, 10));
    let (hour, minute, second) = (number(11, 13), number(14, 16), number(17, 19));
    days_from_civil(year, month, day) * 86_400 + hour * 3_600 + minute * 60 + second
}

/// The days between 1970-01-01 and `year-month-day` (Howard Hinnant's `days_from_civil`, the
/// inverse of the one the writing half converts the clock with).
fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let year = if month <= 2 { year - 1 } else { year };
    let era = year.div_euclid(400);
    let of_era = year - era * 400;
    let month_of_year = (month + 9) % 12;
    let day_of_year = (153 * month_of_year + 2) / 5 + day - 1;
    let day_of_era = of_era * 365 + of_era / 4 - of_era / 100 + day_of_year;
    era * 146_097 + day_of_era - 719_468
}

// ── the seam between create and the editor ──────────────────────────────────────────────────

/// The decks the seam is proved over: two slides with a title on each, notes, a list and a table, so
/// the editor has text it can address on more than one slide.
fn seam_decks() -> Vec<(&'static str, Value)> {
    vec![(
        "seam",
        json!([
            { "type": "heading", "level": 1, "text": "Opening title" },
            { "type": "paragraph", "text": "A body paragraph." },
            { "type": "notes", "text": "A speaker note." },
            { "type": "heading", "level": 1, "text": "Second title" },
            { "type": "list", "items": ["alpha", "beta"] },
            { "type": "table", "headers": ["A", "B"], "rows": [["1", "2"]] },
        ]),
    )]
}

#[test]
fn a_created_deck_that_compares_equal_is_opened_by_the_crate_editor() {
    // This is the attribution rule, and the only place it can be applied: the crate's isolated copy
    // has no reference writer, so a crate-side test cannot first show that a created deck compares
    // equal. Here both engines produce the same request, the two decks are compared with the model,
    // and **only a deck that compares equal is fed to the editor**. A deck that does not compare
    // equal is `create`'s defect and the assertion below names it as such; one that does and cannot
    // be edited is the editor's, and the edit's own answer is reported against the editor.
    let scratch = tempfile::tempdir().expect("a temporary directory");
    let reference =
        ReferenceWriter::new(&scratch.path().join("engine")).expect("the reference writer");
    let crate_writer = CrateWriter;
    for (name, content) in seam_decks() {
        let theirs = produced(
            &reference,
            &scratch.path().join("reference"),
            name,
            &content,
            None,
        );
        let ours = produced(
            &crate_writer,
            &scratch.path().join("crate"),
            name,
            &content,
            None,
        );

        let expected = canonicalize(Family::Pptx, &theirs).expect("the reference's deck");
        let actual = canonicalize(Family::Pptx, &ours).expect("the crate's deck");
        let comparison = compare(&expected, &actual);
        assert!(
            comparison.is_same(),
            "{name}: the created deck does not compare equal to the reference's, so the editor is \
             not at fault and is not exercised: {:?}",
            comparison.lines(8)
        );

        // The deck is now the editor's input. It is written where the editor can open it.
        let input = scratch
            .path()
            .join("crate")
            .join(name)
            .join(format!("{name}.pptx"));
        let directory = scratch.path().join("edited").join(name);
        std::fs::create_dir_all(&directory).expect("a destination");
        let outcome = execute(&Request {
            operation: "pptx_edit".to_string(),
            values: json!({
                "input": input.to_string_lossy(),
                "edits": [{ "op": "replace_text", "slide": 2, "find": "alpha", "replace": "gamma" }],
            })
            .as_object()
            .expect("a request object")
            .clone(),
            destination: Destination {
                directory: directory.clone(),
                base_name: "edited".to_string(),
            },
        });
        assert!(
            outcome.failure.is_none(),
            "{name}: the deck compares equal to the reference's, so the editor is at fault: {:?}",
            outcome.failure
        );
        let edited = std::fs::read(directory.join("edited.pptx")).expect("the edited deck");
        let text = part(&edited, "ppt/slides/slide2.xml");
        let text = String::from_utf8_lossy(&text).into_owned();
        assert!(
            text.contains("gamma"),
            "{name}: the replacement is not on the slide"
        );
        assert!(
            !text.contains("alpha"),
            "{name}: the replaced text is still there"
        );
        // The parts the edit did not name are carried through, which is the seam's promise.
        assert!(
            String::from_utf8_lossy(&part(&edited, "ppt/slides/slide1.xml"))
                .contains("Opening title"),
            "{name}: the first slide lost its title"
        );
    }
}

// ── the media chain, which the model normalises away ────────────────────────────────────────

#[test]
fn a_jpg_and_a_jpeg_source_both_arrive_as_a_jpeg_media_part() {
    // The model replaces a media part's generated name with one derived from its content and keeps
    // only the extension, so it cannot say which extension the writer chose. The chain the
    // reference implements is source extension → content type → media extension, and this is the
    // oracle it has: the same JPEG bytes under both spellings, the same media part, and a content
    // types part that declares `jpeg` and not `jpg`.
    let scratch = tempfile::tempdir().expect("a temporary directory");
    let crate_writer = CrateWriter;
    for source in ["picture.jpg", "picture.jpeg"] {
        let name = source.replace('.', "-");
        let input = scratch.path().join(source);
        std::fs::write(&input, build::jpeg()).expect("the picture");
        let deck = produced(
            &crate_writer,
            &scratch.path().join("crate"),
            &name,
            &json!([{ "type": "image", "image": input.to_string_lossy() }]),
            None,
        );
        let media: Vec<String> = entries(&deck)
            .into_iter()
            .filter(|entry| entry.starts_with("ppt/media/"))
            .collect();
        assert_eq!(media.len(), 1, "{source}: one media part, {media:?}");
        assert!(
            media[0].ends_with(".jpeg"),
            "{source}: the media part keeps the content type's extension: {media:?}"
        );
        let types = String::from_utf8_lossy(&part(&deck, "[Content_Types].xml")).into_owned();
        assert!(types.contains(r#"Extension="jpeg""#), "{source}: {types}");
        assert!(
            !types.contains(r#"Extension="jpg""#),
            "{source}: the default no part uses is pruned: {types}"
        );
    }
}

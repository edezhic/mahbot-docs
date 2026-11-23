//! What a created deck's package says about itself, beyond what the recorded model compares.
//!
//! The canonical model normalises away the two moments a deck is dated with, drops the revision,
//! and excludes `docProps/app.xml` entirely, so a created deck whose dates are a local time wearing
//! a `Z`, whose revision is any number at all, or whose application properties state a different
//! slide count, notes count, title list or format string would compare equal to a wrong deck under
//! that model. These tests pin those things instead, outside the recorded baseline.
//!
//! Three claims:
//!
//! - [`the_core_properties_are_the_frozen_strings_with_this_runs_utc_dates`]: the library's own
//!   fixed strings, its revision of 1, and two stamps that are in the shape the format states, mean
//!   UTC, and sit within a stated tolerance of this run's clock.
//! - [`the_application_properties_state_the_frozen_counts_titles_and_format`]: the part's slide and
//!   notes counts, its title list and its format string.
//! - [`the_archive_holds_exactly_the_frozen_entries_and_no_folders`]: the archive's entry set, so a
//!   part the writer omits or adds is visible, and no folder entry is written.
//!
//! Each value these tests assert was the kit's output and is now a frozen expectation: the kit has
//! retired, so nothing can re-derive it, and a change here is a deliberate replacement.

use crate::corpus::build;
use crate::engine::Writer;
use crate::pptx_capture;
use crate::reference::CrateWriter;
use document_engine::operation::{Destination, Request, execute};
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
fn the_core_properties_are_the_frozen_strings_with_this_runs_utc_dates() {
    // The library's fixed strings were the kit's output and are now a frozen expectation: the kit
    // has retired, so nothing re-derives them. The dates are this run's clock, not a frozen value.
    let scratch = tempfile::tempdir().expect("a temporary directory");
    let crate_writer = CrateWriter;

    for (name, content) in core_decks() {
        let ours = produced(
            &crate_writer,
            &scratch.path().join("crate"),
            name,
            &content,
            None,
        );
        let core = content_of(&part(&ours, "docProps/core.xml"));
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
                "{name}: {element}"
            );
        }
        let now = now_seconds();
        for element in ["dcterms:created", "dcterms:modified"] {
            let stamp = element_text(&core, element)
                .unwrap_or_else(|| panic!("{name}: the deck states no {element}"));
            let seconds = utc_seconds(&stamp);
            assert!(
                (seconds - now).abs() <= TOLERANCE_SECS,
                "{name}: {element} is {stamp}, which is not within {TOLERANCE_SECS}s of this run: \
                 a local time wearing a `Z` is out by the machine's zone offset"
            );
            // The attribute is what makes the text a W3CDTF stamp rather than a string of digits: a
            // reading that ignored it would be reading an untyped element.
            assert_eq!(
                attribute_of(&core, element, "xsi:type").as_deref(),
                Some("dcterms:W3CDTF"),
                "{name}: {element} states no W3CDTF type"
            );
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
fn the_application_properties_state_the_frozen_counts_titles_and_format() {
    // The counts, the title list and the format string were the kit's output and are now frozen
    // expectations. The part is compared by content, because it is content and not bytes that must
    // agree: the elements and their text are what is pinned, not the whitespace between them.
    let scratch = tempfile::tempdir().expect("a temporary directory");
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
        let ours = produced(
            &crate_writer,
            &scratch.path().join("crate"),
            name,
            &content,
            size,
        );
        let ours = content_of(&part(&ours, "docProps/app.xml"));

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
        // says 16:9 too. This pins the value, so a change to the string is visible as itself.
        assert_eq!(
            element_text(&ours, "PresentationFormat").as_deref(),
            Some("On-screen Show (16:9)"),
            "{name}: the format string"
        );
    }
}

// ── the archive's entries ───────────────────────────────────────────────────────────────────

#[test]
fn the_archive_holds_exactly_the_frozen_entries_and_no_folders() {
    // The entry list was the kit's output and is now a frozen expectation. The set is the whole
    // archive: a part the writer omits or adds is visible as a difference here.
    const ENTRIES: &[&str] = &[
        "[Content_Types].xml",
        "_rels/.rels",
        "docProps/app.xml",
        "docProps/core.xml",
        "ppt/_rels/presentation.xml.rels",
        "ppt/notesMasters/_rels/notesMaster1.xml.rels",
        "ppt/notesMasters/notesMaster1.xml",
        "ppt/notesSlides/_rels/notesSlide1.xml.rels",
        "ppt/notesSlides/notesSlide1.xml",
        "ppt/presProps.xml",
        "ppt/presentation.xml",
        "ppt/slideLayouts/_rels/slideLayout1.xml.rels",
        "ppt/slideLayouts/slideLayout1.xml",
        "ppt/slideMasters/_rels/slideMaster1.xml.rels",
        "ppt/slideMasters/slideMaster1.xml",
        "ppt/slides/_rels/slide1.xml.rels",
        "ppt/slides/_rels/slide2.xml.rels",
        "ppt/slides/slide1.xml",
        "ppt/slides/slide2.xml",
        "ppt/tableStyles.xml",
        "ppt/theme/theme1.xml",
        "ppt/viewProps.xml",
    ];
    let scratch = tempfile::tempdir().expect("a temporary directory");
    let crate_writer = CrateWriter;
    let content = json!([
        { "type": "heading", "level": 1, "text": "The opening slide" },
        { "type": "paragraph", "text": "Its body." },
        { "type": "notes", "text": "A speaker note." },
        { "type": "heading", "level": 1, "text": "The closing slide" },
        { "type": "table", "headers": ["Left"], "rows": [["Right"]] },
    ]);

    let ours = produced(
        &crate_writer,
        &scratch.path().join("crate"),
        "entries",
        &content,
        Some("standard"),
    );
    let ours_entries = entries(&ours);
    let frozen: BTreeSet<String> = ENTRIES.iter().map(|name| (*name).to_string()).collect();

    assert_eq!(
        ours_entries, frozen,
        "the archive's entries are not the frozen set"
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
fn a_created_deck_is_opened_by_the_crate_editor() {
    // The seam between create and the editor: a deck the crate creates is one its own editor can
    // open and edit. The kit used to be the oracle for "the deck compares equal"; that equality is
    // now covered by the recorded baseline's `pptx/create` cases, so this test keeps the half the
    // baseline cannot reach — that the created deck is a valid input to `pptx_edit`.
    let scratch = tempfile::tempdir().expect("a temporary directory");
    let crate_writer = CrateWriter;
    for (name, content) in seam_decks() {
        produced(
            &crate_writer,
            &scratch.path().join("crate"),
            name,
            &content,
            None,
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
            "{name}: the crate created the deck and its editor could not open it: {:?}",
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
    // kit implements is source extension → content type → media extension, and this is the
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

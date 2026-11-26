//! The rendering surface, exercised through its public entry point: every family turns into its
//! pages, a page carries a picture and a size, a refusal is classed, the bounds are the vocabulary's,
//! and the answer is deterministic.
//!
//! The per-family layout is exercised in each arm's own tests; this file proves the surface and the
//! promises that cross the families.

use std::path::{Path, PathBuf};

use document_engine::operation::{Destination, Request, execute};
use document_engine::reading::{self, Reading};
use document_engine::render::{Rendered, RenderedPage, render};
use document_engine::test_support::marks::sentence;
use document_engine::test_support::{build, coverage, documents};
use document_engine::vocabulary::{Producer, vocabulary};

/// A temporary directory the test owns.
fn scratch() -> tempfile::TempDir {
    tempfile::TempDir::new().expect("a temporary directory")
}

/// Write `bytes` under `name` in `directory`, answering the path.
fn write(directory: &Path, name: &str, bytes: &[u8]) -> PathBuf {
    let path = directory.join(name);
    std::fs::write(&path, bytes).expect("a temporary document");
    path
}

/// Render one document, requiring its pages.
fn pages(path: &Path, name: &str) -> (document_engine::vocabulary::Family, Vec<RenderedPage>) {
    match render(path, name) {
        Rendered::Pages { family, pages, .. } => (family, pages),
        Rendered::Refused(failure) => {
            panic!(
                "{}: refused {}: {}",
                path.display(),
                failure.class,
                failure.reason
            );
        }
    }
}

/// The text a reading of the same document states: the render's words are compared with it rather
/// than with a second copy of them written here.
fn reading_text(path: &Path, name: &str, out_dir: &Path) -> String {
    match document_engine::reading::read(path, name, out_dir) {
        document_engine::reading::Reading::Text { text, .. } => text,
        document_engine::reading::Reading::Unreadable { reason } => {
            panic!("{}: the reading refused it: {reason}", path.display())
        }
        document_engine::reading::Reading::Unsupported => {
            panic!("{}: the reading does not read it", path.display())
        }
    }
}

/// Every word a page draws is one the reading of the same document states.
///
/// Only the words are compared: a list marker, a footnote's number and a field's result are the
/// layout's own additions, and the reading does not state them. A word glued to its neighbour —
/// "paragraphone" for "paragraph one" — is a word the reading does not state, which is the failure
/// this catches.
fn assert_words_are_read(drawn: &[&str], reading: &str) {
    for word in drawn.iter().flat_map(|line| words(line)) {
        assert!(
            reading.contains(&word),
            "the page draws {word:?}, which the reading does not state: {drawn:?}"
        );
    }
}

/// The runs of letters in a line: its words, without the digits and punctuation a layout adds.
fn words(line: &str) -> Vec<String> {
    let mut words = Vec::new();
    let mut word = String::new();
    for character in line.chars() {
        if character.is_alphabetic() {
            word.push(character);
        } else if !word.is_empty() {
            words.push(std::mem::take(&mut word));
        }
    }
    if !word.is_empty() {
        words.push(word);
    }
    words
}

/// The refusal of one document, requiring one.
fn refusal(path: &Path, name: &str) -> (String, String) {
    match render(path, name) {
        Rendered::Refused(failure) => (failure.class, failure.reason),
        Rendered::Pages { pages, .. } => {
            panic!(
                "{}: rendered {} page(s), expected a refusal",
                path.display(),
                pages.len()
            );
        }
    }
}

#[test]
fn every_family_turns_into_its_pages() {
    let directory = scratch();
    for (document, family) in [
        (documents::BASIC_DOCX, "docx"),
        (documents::BASIC_XLSX, "xlsx"),
        (documents::BASIC_PPTX, "pptx"),
        (documents::BASIC_PDF, "pdf"),
    ] {
        let path = write(directory.path(), document.name, &(document.build)());
        let (rendered, pages) = pages(&path, document.name);
        assert_eq!(rendered.as_str(), family, "{}", document.name);
        assert!(
            !pages.is_empty(),
            "{} renders at least one page",
            document.name
        );
        for (index, page) in pages.iter().enumerate() {
            assert_eq!(
                page.number,
                index + 1,
                "{}: pages are numbered",
                document.name
            );
            assert!(page.width_pt > 0.0 && page.height_pt > 0.0);
            let picture = page
                .picture
                .as_ref()
                .unwrap_or_else(|| panic!("{}: page {} has a picture", document.name, page.number));
            assert!(picture.width > 0 && picture.height > 0);
            assert_eq!(
                picture.rgba.len(),
                picture.width as usize * picture.height as usize * 4,
                "{}: page {} is RGBA",
                document.name,
                page.number
            );
        }
    }
}

#[test]
fn a_document_the_engine_wrote_renders_as_it_reads() {
    let directory = scratch();
    let request = Request {
        operation: "create".to_string(),
        values: serde_json::json!({
            "format": "docx",
            "content": [
                {"type": "heading", "text": "Quarterly report", "level": 1},
                {"type": "paragraph", "text": "A paragraph long enough that the column must break it across more than one line, which is what makes a page worth drawing at all."},
                {"type": "table", "headers": ["Region", "Revenue"], "rows": [["North", "1 200"], ["South", "980"]]},
                {"type": "page_break"},
                {"type": "paragraph", "text": "After the break."}
            ],
            "header": "Quarterly report",
            "footer": "Page {page} of {pages}"
        })
        .as_object()
        .expect("an object")
        .clone(),
        destination: Destination {
            directory: directory.path().to_path_buf(),
            base_name: "report".to_string(),
        },
    };
    let outcome = execute(&request);
    let path = outcome.outputs.first().expect("one file").clone();
    let (_, pages) = pages(&path, "report.docx");
    assert_eq!(pages.len(), 2);
    let first: Vec<&str> = pages[0]
        .lines
        .iter()
        .map(|line| line.text.as_str())
        .collect();
    assert!(first.contains(&"Quarterly report"));
    assert!(first.iter().any(|line| line.contains("Region Revenue")));
    assert!(first.iter().any(|line| line.contains("North 1 200")));
    assert!(first.contains(&"Page 1 of 2"));
    let second: Vec<&str> = pages[1]
        .lines
        .iter()
        .map(|line| line.text.as_str())
        .collect();
    assert!(second.contains(&"After the break."));
    assert!(second.contains(&"Page 2 of 2"));
    assert!(first.len() > 4, "the paragraph broke across lines");
}

#[test]
fn an_engine_written_presentation_renders_as_it_reads() {
    let directory = scratch();
    let template = write(directory.path(), "template.pptx", &build::pptx_template());
    let request = Request {
        operation: "fill_template".to_string(),
        values: serde_json::json!({
            "template": template.to_string_lossy(),
            "format": "pptx",
            "values": {"name": "Ada"}
        })
        .as_object()
        .expect("an object")
        .clone(),
        destination: Destination {
            directory: directory.path().to_path_buf(),
            base_name: "filled".to_string(),
        },
    };
    let outcome = execute(&request);
    assert_eq!(outcome.failure, None, "{outcome:?}");
    let path = outcome.outputs.first().expect("one file").clone();
    let (_, pages) = pages(&path, "filled.pptx");
    assert_eq!(pages.len(), 1);
    let lines: Vec<&str> = pages[0]
        .lines
        .iter()
        .map(|line| line.text.as_str())
        .collect();
    assert!(lines.contains(&"Greeting"), "{lines:?}");
    assert!(lines.contains(&"Hello Ada"), "{lines:?}");
    assert!(
        pages[0]
            .unshown
            .iter()
            .all(|line| line.contains("whose frame no part states")),
        "the filled slide draws its text; only the frame it inherits is stated: {:?}",
        pages[0].unshown
    );
}

#[test]
fn a_word_document_renders_the_words_its_reading_gives() {
    // The render's words are the reading's words: every word the page draws is one the reading of
    // the same document states, and the space a document states between two words survives the run
    // joining.
    let directory = scratch();
    let path = write(directory.path(), "basic.docx", &build::docx_basic());
    let (_, pages) = pages(&path, "basic.docx");
    let lines: Vec<&str> = pages
        .iter()
        .flat_map(|page| page.lines.iter())
        .map(|line| line.text.as_str())
        .collect();
    let reading = reading_text(&path, "basic.docx", directory.path());
    assert_words_are_read(&lines, &reading);
    assert!(
        lines
            .iter()
            .any(|line| line.contains("Alpha paragraph one")),
        "{lines:?}"
    );
    assert!(
        lines.iter().any(|line| line.contains("Beta paragraph two")),
        "{lines:?}"
    );
}

#[test]
fn a_workbook_page_draws_the_cells_its_reading_gives() {
    // The render draws the values the reading reads, in the columns the workbook puts them in: the
    // merged range is drawn once, across its whole box, and the cell after it is not moved left.
    let directory = scratch();
    let path = write(directory.path(), "basic.xlsx", &build::xlsx_basic());
    let (_, pages) = pages(&path, "basic.xlsx");
    let reading = reading_text(&path, "basic.xlsx", directory.path());
    let text: Vec<&str> = pages
        .iter()
        .flat_map(|page| page.lines.iter())
        .map(|line| line.text.as_str())
        .collect();
    assert_words_are_read(&text, &reading);
    for word in ["Name", "Value", "Alpha", "Beta", "Merged", "Other"] {
        assert!(
            text.iter().any(|line| line.contains(word)),
            "the page draws {word:?}: {text:?}"
        );
    }
    let merged = text.iter().filter(|line| line.contains("Merged")).count();
    assert_eq!(merged, 1, "a merged range is drawn once: {text:?}");
}

#[test]
fn a_column_range_past_the_sheet_s_last_column_states_nothing_about_it() {
    // The workbook states a hidden range over columns past its only cell, a range whose end cannot be
    // read, and a range past the grid: the reading applies none of them to column A, so the render
    // draws A's cell and names the columns the sheet really hides — not a column that holds data.
    let directory = scratch();
    let path = write(
        directory.path(),
        "column-definitions.xlsx",
        &build::xlsx_column_definitions(),
    );
    let (_, pages) = pages(&path, "column-definitions.xlsx");
    let text: Vec<&str> = pages
        .iter()
        .flat_map(|page| page.lines.iter())
        .map(|line| line.text.as_str())
        .collect();
    let reading = reading_text(&path, "column-definitions.xlsx", directory.path());
    assert!(reading.contains("A1: 1"), "{reading}");
    assert!(
        text.iter().any(|line| line.contains('1')),
        "the sheet's only cell is drawn: {text:?}"
    );
    let named: Vec<&String> = pages.iter().flat_map(|page| page.unshown.iter()).collect();
    assert_eq!(
        named,
        ["the sheet hides 1 column; C is not drawn"],
        "only the column the sheet really hides is named"
    );
}

#[test]
fn a_stated_page_scale_no_page_could_hold_is_drawn_at_the_nearest_one_it_could() {
    // A finite but absurd `pageSetup` scale would put every position past any page and write an
    // exponent into the content stream; the format's own range bounds it, so the page is drawn.
    let directory = scratch();
    let sheet = format!(
        concat!(
            r#"<worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main">"#,
            r#"<pageSetup paperSize="9" scale="{}"/>"#,
            r#"<sheetData><row r="1"><c r="A1"><v>7</v></c></row></sheetData></worksheet>"#,
        ),
        "1e300",
    );
    let bytes = xlsx_workbook(&sheet);
    let path = write(directory.path(), "scale.xlsx", &bytes);
    let (_, pages) = pages(&path, "scale.xlsx");
    assert_eq!(pages.len(), 1);
    assert!(pages[0].picture.is_some(), "the page is drawn");
    assert!(
        pages[0]
            .lines
            .iter()
            .all(|line| line.x_pt.is_finite() && line.x_pt < pages[0].width_pt),
        "{:?}",
        pages[0].lines
    );
}

#[test]
fn a_table_border_is_drawn_in_the_colour_it_states() {
    let directory = scratch();
    let body = concat!(
        r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body>"#,
        r#"<w:tbl><w:tblPr><w:tblBorders>"#,
        r#"<w:top w:val="single" w:sz="24" w:color="FF0000"/>"#,
        r#"<w:left w:val="single" w:sz="24" w:color="0000FF"/>"#,
        r#"</w:tblBorders></w:tblPr><w:tblGrid><w:gridCol w:w="4000"/></w:tblGrid>"#,
        r#"<w:tr><w:tc><w:p><w:r><w:t>cell</w:t></w:r></w:p></w:tc></w:tr></w:tbl>"#,
        r#"<w:sectPr><w:pgSz w:w="11906" w:h="16838"/><w:pgMar w:top="1440" w:right="1440" w:bottom="1440" w:left="1440"/></w:sectPr>"#,
        r"</w:body></w:document>",
    );
    let bytes = zip_parts(&[("word/document.xml", body.as_bytes())]);
    let path = write(directory.path(), "border.docx", &bytes);
    let (_, pages) = pages(&path, "border.docx");
    let picture = pages[0].picture.as_ref().expect("the page is drawn");
    // Both axes are checked: a horizontal border and a vertical one are drawn by different code,
    // and a colour stated for one is the colour of the other.
    assert!(
        picture
            .rgba
            .chunks(4)
            .any(|pixel| pixel[0] > 200 && pixel[1] < 60 && pixel[2] < 60),
        "the top border is drawn in the red the document states"
    );
    assert!(
        picture
            .rgba
            .chunks(4)
            .any(|pixel| pixel[2] > 200 && pixel[0] < 60 && pixel[1] < 60),
        "the left border is drawn in the blue the document states"
    );
}

#[test]
fn a_character_the_carried_face_lacks_is_named_and_a_bullet_is_drawn() {
    // A bullet is drawn as the circle it names, so it is not a gap; a character outside the face's
    // repertoire is named through the answer's unsupported channel rather than drawn as a blank box.
    let directory = scratch();
    let body = format!(
        concat!(
            r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body>"#,
            r"<w:p><w:r><w:t>{} bullet</w:t></w:r></w:p>",
            r"<w:p><w:r><w:t>an arrow {} and a check {}</w:t></w:r></w:p>",
            r#"<w:sectPr><w:pgSz w:w="11906" w:h="16838"/><w:pgMar w:top="1440" w:right="1440" w:bottom="1440" w:left="1440"/></w:sectPr>"#,
            r"</w:body></w:document>",
        ),
        '\u{25CF}', '\u{2192}', '\u{2713}',
    );
    let bytes = zip_parts(&[("word/document.xml", body.as_bytes())]);
    let path = write(directory.path(), "characters.docx", &bytes);
    match render(&path, "characters.docx") {
        Rendered::Pages {
            pages, unsupported, ..
        } => {
            assert_eq!(
                unsupported,
                vec!["\u{2192}".to_string(), "\u{2713}".to_string()]
            );
            let lines: Vec<&str> = pages
                .iter()
                .flat_map(|page| page.lines.iter())
                .map(|line| line.text.as_str())
                .collect();
            assert!(
                lines.iter().any(|line| line.contains("bullet")),
                "{lines:?}"
            );
        }
        Rendered::Refused(failure) => panic!("refused {}: {}", failure.class, failure.reason),
    }
}

#[test]
fn every_document_the_engine_wrote_renders_without_a_blank_box() {
    // The corpus documents whose render is a refusal rather than pages. The list is reviewed here
    // rather than read from the corpus: no corpus document declares that its own render is a
    // refusal, so this test states each one and why the list is exact — a refusal that is not one of
    // these fails it. One document has no page at all, some are its deliberately malformed parts,
    // and some are inputs that are not a package of the family their name claims.
    const STATED_REFUSALS: [&str; 7] = [
        "bodyless.docx",
        "broken.docx",
        "fill-archive.docx",
        "fill-invalid-xml.docx",
        "fill-unreadable.docx",
        "fill-unreadable.xlsx",
        "no-family.xlsx",
    ];
    // The face the crate carries covers everything the writer can emit — its bullets included — so a
    // page of the engine's own output shows no placeholder: the answer names no undrawable character.
    let directory = scratch();
    let mut checked = 0_usize;
    let mut refused: Vec<&str> = Vec::new();
    for document in documents::writer_documents() {
        let Some(extension) = extension(document.name) else {
            continue;
        };
        if !matches!(
            extension,
            "docx" | "docm" | "pptx" | "pptm" | "xlsx" | "xlsm"
        ) {
            continue;
        }
        let path = write(directory.path(), document.name, &(document.build)());
        let unsupported = match render(&path, document.name) {
            Rendered::Pages { unsupported, .. } => unsupported,
            // A refusal is collected and compared with the stated list below, so a document the
            // renderer refuses is never skipped in silence.
            Rendered::Refused(_) => {
                refused.push(document.name);
                continue;
            }
        };
        checked += 1;
        assert!(
            unsupported.is_empty(),
            "{}: the engine's own output names {unsupported:?}, which its face cannot draw",
            document.name,
        );
    }
    refused.sort_unstable();
    assert_eq!(
        refused, STATED_REFUSALS,
        "the corpus's own documents are refused only for the reasons it states"
    );
    assert!(
        checked > 20,
        "the promise is checked over the corpus's own documents ({checked})"
    );
}

#[test]
fn a_paragraph_of_tabs_is_bounded_and_the_rest_is_named() {
    // A right-aligned tab measures what follows it, so a paragraph of tabs is work a few kilobytes
    // of input could otherwise ask for without limit. The bound is the vocabulary's, and the tabs
    // past it are named rather than drawn as text shifted by an unnamed loss.
    let directory = scratch();
    let held = vocabulary().render.tabs_max;
    let mut tabs = String::new();
    for _ in 0..(held + 2) {
        tabs.push_str(r#"<w:r><w:tab/></w:r>"#);
    }
    let body = format!(
        concat!(
            r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body>"#,
            r"<w:p>{}</w:p>",
            r#"<w:sectPr><w:pgSz w:w="11906" w:h="16838"/><w:pgMar w:top="1440" w:right="1440" w:bottom="1440" w:left="1440"/></w:sectPr>"#,
            r"</w:body></w:document>",
        ),
        tabs,
    );
    let bytes = zip_parts(&[("word/document.xml", body.as_bytes())]);
    let path = write(directory.path(), "tabs.docx", &bytes);
    let (_, pages) = pages(&path, "tabs.docx");
    let named = vocabulary()
        .marks
        .format("render_tabs_truncated", &[("max", &held.to_string())]);
    assert!(
        pages.iter().any(|page| page.unshown.contains(&named)),
        "the tabs past the bound are named: {pages:?}"
    );
}

#[test]
fn a_picture_inside_a_document_is_drawn_on_its_page() {
    // The document's own picture is drawn where the document anchors it, so the page describes a
    // picture as well as its text.
    let directory = scratch();
    let path = write(
        directory.path(),
        "picture.docx",
        &(documents::PICTURE_DOCX.build)(),
    );
    let (_, pages) = pages(&path, "picture.docx");
    assert!(
        pages.iter().any(|page| !page.pictures.is_empty()),
        "the page draws the document's picture"
    );
}

#[test]
fn a_picture_past_the_bound_is_a_page_s_gap_and_the_document_still_renders() {
    // A PNG whose header states a side no decoder could allocate is a header bomb, not a picture:
    // the page still draws its text, and the picture is counted as not drawn rather than the whole
    // document being refused over it.
    let directory = scratch();
    let bomb = header_bomb_png();
    let body = "<w:document xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\"                 xmlns:r=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships\"                 xmlns:wp=\"http://schemas.openxmlformats.org/drawingml/2006/wordprocessingDrawing\"                 xmlns:a=\"http://schemas.openxmlformats.org/drawingml/2006/main\"                 xmlns:pic=\"http://schemas.openxmlformats.org/drawingml/2006/picture\">                <w:body><w:p><w:r><w:t>Beside the picture</w:t></w:r></w:p>                <w:p><w:r><w:drawing><wp:inline><wp:extent cx=\"914400\" cy=\"914400\"/>                <a:graphic><a:graphicData><pic:pic><pic:blipFill><a:blip r:embed=\"rId1\"/>                </pic:blipFill></pic:pic></a:graphicData></a:graphic></wp:inline></w:drawing></w:r>                </w:p><w:sectPr><w:pgSz w:w=\"11906\" w:h=\"16838\"/>                <w:pgMar w:top=\"1440\" w:right=\"1440\" w:bottom=\"1440\" w:left=\"1440\"/>                </w:sectPr></w:body></w:document>";
    let rels = "<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\">                <Relationship Id=\"rId1\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/image\"                 Target=\"media/bomb.png\"/></Relationships>";
    let bytes = zip_parts(&[
        ("word/document.xml", body.as_bytes()),
        ("word/_rels/document.xml.rels", rels.as_bytes()),
        ("word/media/bomb.png", &bomb),
    ]);
    let path = write(directory.path(), "bomb.docx", &bytes);
    let (_, pages) = pages(&path, "bomb.docx");
    let drawn = pages.iter().flat_map(|page| page.pictures.iter()).count();
    assert_eq!(drawn, 0, "no picture is drawn");
    assert!(
        pages
            .iter()
            .flat_map(|page| page.unshown.iter())
            .any(|line| line.contains("picture") && line.contains("not drawn")),
        "{:?}",
        pages.iter().map(|page| &page.unshown).collect::<Vec<_>>()
    );
    assert!(
        pages.iter().any(|page| !page.lines.is_empty()),
        "the page still draws its text"
    );
}

#[test]
fn a_word_header_is_drawn_on_the_pages_it_belongs_to() {
    // A section stating a different first page draws that page's header, and a document stating that
    // its even pages are drawn differently draws theirs: the header a page belongs to is the header
    // it gets, and a substitution is never silent.
    let directory = scratch();
    let body = concat!(
        r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main" "#,
        r#"xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships">"#,
        r#"<w:body><w:p><w:r><w:t>First page</w:t></w:r></w:p>"#,
        r#"<w:p><w:r><w:br w:type="page"/></w:r><w:r><w:t>Second page</w:t></w:r></w:p>"#,
        r#"<w:p><w:r><w:br w:type="page"/></w:r><w:r><w:t>Third page</w:t></w:r></w:p>"#,
        r#"<w:sectPr><w:titlePg/>"#,
        r#"<w:headerReference w:type="default" r:id="rId4"/>"#,
        r#"<w:headerReference w:type="first" r:id="rId5"/>"#,
        r#"<w:headerReference w:type="even" r:id="rId6"/>"#,
        r#"<w:pgSz w:w="11906" w:h="16838"/>"#,
        r#"<w:pgMar w:top="1440" w:right="1440" w:bottom="1440" w:left="1440"/>"#,
        r#"</w:sectPr></w:body></w:document>"#,
    );
    let header = |text: &str| {
        format!(
            "<w:hdr xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\">\
             <w:p><w:r><w:t>{text}</w:t></w:r></w:p></w:hdr>"
        )
    };
    let rels = concat!(
        r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">"#,
        r#"<Relationship Id="rId4" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/header" Target="header1.xml"/>"#,
        r#"<Relationship Id="rId5" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/header" Target="header2.xml"/>"#,
        r#"<Relationship Id="rId6" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/header" Target="header3.xml"/>"#,
        r#"</Relationships>"#,
    );
    let settings = concat!(
        r#"<w:settings xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">"#,
        r#"<w:evenAndOddHeaders/></w:settings>"#,
    );
    let bytes = zip_parts(&[
        ("word/document.xml", body.as_bytes()),
        ("word/_rels/document.xml.rels", rels.as_bytes()),
        ("word/settings.xml", settings.as_bytes()),
        ("word/header1.xml", header("Body header").as_bytes()),
        ("word/header2.xml", header("First header").as_bytes()),
        ("word/header3.xml", header("Even header").as_bytes()),
    ]);
    let path = write(directory.path(), "headers.docx", &bytes);
    let (_, pages) = pages(&path, "headers.docx");
    assert_eq!(pages.len(), 3, "{:?}", pages.len());
    let texts = |page: &RenderedPage| -> Vec<String> {
        page.lines.iter().map(|line| line.text.clone()).collect()
    };
    let has = |page: &RenderedPage, text: &str| texts(page).iter().any(|line| line.contains(text));
    assert!(has(&pages[0], "First header"), "{:?}", texts(&pages[0]));
    assert!(!has(&pages[0], "Body header"), "{:?}", texts(&pages[0]));
    assert!(has(&pages[1], "Even header"), "{:?}", texts(&pages[1]));
    assert!(!has(&pages[1], "Body header"), "{:?}", texts(&pages[1]));
    assert!(has(&pages[2], "Body header"), "{:?}", texts(&pages[2]));
}

/// A PNG whose header states a side past any bound, with no image data at all: what a decoder would
/// allocate from it is the reason the render refuses to draw it.
fn header_bomb_png() -> Vec<u8> {
    /// The side the header claims, in pixels.
    const SIDE: u32 = 200_000;
    let mut bytes = vec![0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a];
    let mut chunk = |kind: &[u8], data: &[u8]| {
        bytes.extend_from_slice(&(data.len() as u32).to_be_bytes());
        bytes.extend_from_slice(kind);
        bytes.extend_from_slice(data);
        bytes.extend_from_slice(&[0, 0, 0, 0]);
    };
    let mut header = SIDE.to_be_bytes().to_vec();
    header.extend_from_slice(&SIDE.to_be_bytes());
    header.extend_from_slice(&[8, 2, 0, 0, 0]);
    chunk(b"IHDR", &header);
    bytes
}

#[test]
fn a_page_picture_has_ink_where_the_document_draws_some() {
    let directory = scratch();
    let path = write(directory.path(), "basic.docx", &build::docx_basic());
    let (_, pages) = pages(&path, "basic.docx");
    let picture = pages[0].picture.as_ref().expect("a picture");
    assert!(
        picture.rgba.chunks(4).any(|pixel| pixel[0] < 128),
        "the page draws its text, not blank paper"
    );
}

#[test]
fn a_page_picture_is_deterministic() {
    let directory = scratch();
    let path = write(directory.path(), "basic.docx", &build::docx_basic());
    let (_, one) = pages(&path, "basic.docx");
    let (_, other) = pages(&path, "basic.docx");
    assert_eq!(one.len(), other.len());
    for (left, right) in one.iter().zip(&other) {
        assert_eq!(left.width_pt, right.width_pt);
        assert_eq!(left.height_pt, right.height_pt);
        assert_eq!(left.lines, right.lines);
        assert_eq!(left.faces, right.faces);
        let left = left.picture.as_ref().expect("a picture");
        let right = right.picture.as_ref().expect("a picture");
        assert_eq!(left.width, right.width);
        assert_eq!(left.height, right.height);
        assert_eq!(
            left.rgba, right.rgba,
            "the same document draws the same page"
        );
    }
}

#[test]
fn a_legacy_document_is_a_stated_exception() {
    let directory = scratch();
    let path = write(
        directory.path(),
        "old.doc",
        &(documents::LEGACY_DOC.build)(),
    );
    let (class, reason) = refusal(&path, "old.doc");
    assert_eq!(class, "usage");
    assert!(reason.contains("legacy"), "{reason}");
}

#[test]
fn a_file_the_engine_cannot_read_is_refused_with_a_class() {
    let directory = scratch();
    let path = write(directory.path(), "broken.docx", b"not a package");
    let (class, reason) = refusal(&path, "broken.docx");
    assert_eq!(class, "usage");
    assert!(!reason.is_empty());
    // An encrypted package is refused by name.
    let encrypted = write(directory.path(), "secret.docx", &build::encrypted_package());
    let (class, reason) = refusal(&encrypted, "secret.docx");
    assert_eq!(class, "usage");
    assert!(reason.contains("password"), "{reason}");
    // A kind the engine does not render.
    let plain = write(directory.path(), "notes.txt", b"just words");
    let (class, reason) = refusal(&plain, "notes.txt");
    assert_eq!(class, "usage");
    assert!(reason.contains("renderer"), "{reason}");
}

#[test]
fn a_document_with_a_chart_renders_and_names_the_gap() {
    let directory = scratch();
    let path = write(
        directory.path(),
        "chart.docx",
        &(documents::CHART_DOCX.build)(),
    );
    let Rendered::Pages { unshown, .. } = render(&path, "chart.docx") else {
        panic!("chart.docx renders");
    };
    // A chart is a part of the package, so its loss is the document's: one sentence however many
    // drawings name it.
    assert!(
        unshown.iter().any(|line| line.contains("chart")),
        "the chart is named in the document's report: {unshown:?}"
    );
}

#[test]
fn a_workbook_names_its_embedded_object_once_and_its_frames_on_their_page() {
    // One anchored embedded object in a workbook is one loss, of kind object: not also an embedded
    // drawing, and not also the object's preview image as a separate drawing. A vector drawing part
    // is one document loss too; the raster parts the workbook cannot embed are the page's.
    let directory = scratch();
    let path = write(
        directory.path(),
        "media.xlsx",
        &(documents::MEDIA_XLSX.build)(),
    );
    let Rendered::Pages { pages, unshown, .. } = render(&path, "media.xlsx") else {
        panic!("media.xlsx renders");
    };
    let document: Vec<&str> = unshown.iter().map(String::as_str).collect();
    for line in [
        "1 embedded object part not drawn",
        "1 embedded drawing not drawn",
        "1 chart not drawn",
    ] {
        assert!(
            document.contains(&line),
            "the document report names {line:?}: {document:?}"
        );
    }
    let page: Vec<&str> = pages
        .iter()
        .flat_map(|page| page.unshown.iter().map(String::as_str))
        .collect();
    assert_eq!(
        page,
        vec!["3 frames whose part is not drawn", "2 pictures not drawn"],
        "the frames whose vector, chart and diagram parts the package lacks, and the two rasters it \
         cannot embed, are the page's own losses"
    );
}

#[test]
fn a_workbook_of_empty_sheets_still_draws_its_pages_and_names_what_it_left_out() {
    // A worksheet stating no cell is still a page: the reading states the sheet, so the render must
    // not refuse the workbook as having no pages. The sheets past the bound are named, as many as one
    // report line lists and a count of the rest.
    let directory = scratch();
    let path = write(
        directory.path(),
        "many-sheets.xlsx",
        &(documents::MANY_SHEETS_XLSX.build)(),
    );
    let Rendered::Pages { pages, unshown, .. } = render(&path, "many-sheets.xlsx") else {
        panic!("many-sheets.xlsx renders");
    };
    assert_eq!(
        pages.len(),
        vocabulary().render.pages_max,
        "one page per empty sheet, up to the bound"
    );
    let line = unshown
        .iter()
        .find(|line| line.contains("were not drawn"))
        .expect("the truncation is named");
    let max = vocabulary().render.pages_max;
    assert!(line.contains(&format!("the first {max}")), "{line}");
    assert!(line.contains("S1000"), "{line}");
    assert!(line.ends_with("and 3 more"), "{line}");
    assert!(
        !line.contains("S1008"),
        "the line names no more sheets than it may list: {line}"
    );
}

#[test]
fn a_word_header_kind_with_no_part_draws_none() {
    // The section states a different first page and the settings draw even pages differently, while
    // only the body's header and footer are referenced: the first and the even page draw neither,
    // and the third page draws both.
    let directory = scratch();
    let path = write(
        directory.path(),
        "header-kinds.docx",
        &(documents::HEADER_KINDS_DOCX.build)(),
    );
    let (_, pages) = pages(&path, "header-kinds.docx");
    assert_eq!(pages.len(), 3, "three pages");
    let drawn =
        |page: &RenderedPage, text: &str| page.lines.iter().any(|line| line.text.contains(text));
    for (index, text) in [
        (0, "First page body"),
        (1, "Second page body"),
        (2, "Third page body"),
    ] {
        assert!(
            drawn(&pages[index], text),
            "page {} draws {text}",
            index + 1
        );
    }
    for page in &pages[..2] {
        assert!(
            !drawn(page, "Body header") && !drawn(page, "Body footer"),
            "a stated kind of page with no part for it draws none: {:?}",
            page.lines.iter().map(|line| &line.text).collect::<Vec<_>>()
        );
    }
    assert!(
        drawn(&pages[2], "Body header"),
        "the third page draws the body's header"
    );
    assert!(
        drawn(&pages[2], "Body footer"),
        "the third page draws the body's footer"
    );
}

#[test]
fn a_workbook_formula_is_named_not_drawn() {
    let directory = scratch();
    let request = Request {
        operation: "create".to_string(),
        values: serde_json::json!({
            "format": "xlsx",
            "sheets": [{"name": "Sheet1", "rows": [["A", "B"], ["one", 1], ["two", 2]]}]
        })
        .as_object()
        .expect("an object")
        .clone(),
        destination: Destination {
            directory: directory.path().to_path_buf(),
            base_name: "book".to_string(),
        },
    };
    let created = execute(&request).outputs.first().expect("one file").clone();
    let edit = Request {
        operation: "xlsx_edit".to_string(),
        values: serde_json::json!({
            "input": created,
            "edits": [{"op": "set_cell", "sheet": "Sheet1", "cell": "B4", "value": {"formula": "=SUM(B2:B3)"}}]
        })
        .as_object()
        .expect("an object")
        .clone(),
        destination: Destination {
            directory: directory.path().to_path_buf(),
            base_name: "book-2".to_string(),
        },
    };
    let path = execute(&edit).outputs.first().expect("one file").clone();
    match render(&path, "book-2.xlsx") {
        Rendered::Pages { pages, notes, .. } => {
            let named: Vec<&str> = pages
                .iter()
                .flat_map(|page| page.unshown.iter().map(String::as_str))
                .collect();
            assert!(
                named.iter().any(|line| line.contains("formula")),
                "the formula value is named: {named:?}"
            );
            assert!(
                notes.iter().any(|note| note.contains("Calibri")),
                "the substituted face is named: {notes:?}"
            );
        }
        Rendered::Refused(failure) => panic!("refused {}: {}", failure.class, failure.reason),
    }
}

#[test]
fn a_page_past_the_bound_is_refused() {
    let directory = scratch();
    let path = write(
        directory.path(),
        "huge.pdf",
        &pdf_with_pages(1, 20_000.0, 20_000.0),
    );
    let (class, reason) = refusal(&path, "huge.pdf");
    assert_eq!(class, "usage");
    assert!(reason.contains("points"), "{reason}");
}

#[test]
fn a_page_of_no_size_is_refused_with_its_own_sentence() {
    // A page of nothing is a page the document does not state, not one it states too large: the two
    // are refused with different sentences, so a reader knows which fault to fix.
    let directory = scratch();
    let body = concat!(
        r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">"#,
        r#"<w:body><w:p><w:r><w:t>No page</w:t></w:r></w:p>"#,
        r#"<w:sectPr><w:pgSz w:w="0" w:h="0"/></w:sectPr></w:body></w:document>"#
    );
    let path = write(
        directory.path(),
        "empty.docx",
        &zip_parts(&[("word/document.xml", body.as_bytes())]),
    );
    let (class, reason) = refusal(&path, "empty.docx");
    assert_eq!(class, "usage");
    assert!(reason.contains("no page at all"), "{reason}");
}

#[test]
fn a_stated_page_count_past_the_bound_is_refused() {
    let directory = scratch();
    let bound = vocabulary().render.pages_max;
    let path = write(
        directory.path(),
        "many.pdf",
        &pdf_with_pages(bound + 1, 200.0, 200.0),
    );
    let (class, reason) = refusal(&path, "many.pdf");
    assert_eq!(class, "usage");
    assert!(reason.contains(&(bound + 1).to_string()), "{reason}");
}

#[test]
fn a_document_with_no_page_is_refused() {
    let directory = scratch();
    let path = write(
        directory.path(),
        "empty.pdf",
        &pdf_with_pages(0, 200.0, 200.0),
    );
    let (class, reason) = refusal(&path, "empty.pdf");
    assert_eq!(class, "usage");
    assert!(reason.contains("no page"), "{reason}");
}

#[test]
fn a_computed_page_count_is_stopped_at_the_bound() {
    let directory = scratch();
    let bound = vocabulary().render.pages_max;
    let mut breaks = String::new();
    for _ in 0..=bound {
        breaks.push_str("<w:br w:type=\"page\"/>");
    }
    let body = format!(
        "<w:document xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\"><w:body><w:p><w:r>{breaks}</w:r></w:p><w:sectPr><w:pgSz w:w=\"11906\" w:h=\"16838\"/><w:pgMar w:top=\"1440\" w:right=\"1440\" w:bottom=\"1440\" w:left=\"1440\"/></w:sectPr></w:body></w:document>"
    );
    let bytes = zip_parts(&[("word/document.xml", body.as_bytes())]);
    let path = write(directory.path(), "breaks.docx", &bytes);
    match render(&path, "breaks.docx") {
        Rendered::Pages { pages, unshown, .. } => {
            assert_eq!(pages.len(), bound);
            assert!(
                unshown.iter().any(|line| line.contains("past the first")),
                "{unshown:?}"
            );
            let undrawn = pages.iter().filter(|page| page.picture.is_none()).count();
            assert!(undrawn > 0, "the picture budget leaves later pages undrawn");
            assert!(
                pages
                    .iter()
                    .flat_map(|page| page.unshown.iter())
                    .any(|line| line.contains("bytes its pictures may hold")),
                "the budget is named"
            );
        }
        Rendered::Refused(failure) => panic!("refused {}: {}", failure.class, failure.reason),
    }
}

#[test]
fn every_rendering_mark_is_readable() {
    let marks = &vocabulary().marks;
    let mut count = 0;
    for (key, text) in marks.printed(Producer::Rendering) {
        assert!(!text.is_empty(), "`{key}` prints an empty mark");
        assert_eq!(marks.get(key), text, "`{key}` is readable");
        count += 1;
    }
    assert!(count > 0, "the renderer prints at least one mark");
}

#[test]
fn every_rendering_mark_is_witnessed_or_recorded_unexercised() {
    // The marks a render call can print. Each is either witnessed by a corpus document — this test
    // renders that document and finds the rendering in its answer — or recorded with why no corpus
    // document can raise it, like the reading's and the writing's own ledgers.
    let all: std::collections::BTreeSet<&str> = vocabulary()
        .marks
        .printed(Producer::Rendering)
        .map(|(key, _)| key)
        .collect();
    let mut covered: std::collections::BTreeSet<&str> = std::collections::BTreeSet::new();
    let mut answers: std::collections::BTreeMap<&str, Vec<String>> =
        std::collections::BTreeMap::new();
    let directory = scratch();
    for entry in coverage::RENDER_WITNESSES {
        assert!(
            all.contains(entry.mark),
            "`{}` is not a mark the renderer prints",
            entry.mark
        );
        assert!(
            covered.insert(entry.mark),
            "`{}` is listed twice",
            entry.mark
        );
        let strings = answers
            .entry(entry.document)
            .or_insert_with(|| render_strings(&directory, entry.document));
        assert!(
            strings.iter().any(|line| line.contains(entry.witness)),
            "`{}`: {} does not print {:?} in {strings:?}",
            entry.mark,
            entry.document,
            entry.witness
        );
    }
    for entry in coverage::UNRENDERED {
        for mark in entry.marks {
            assert!(
                all.contains(mark),
                "`{mark}` is not a mark the renderer prints"
            );
            assert!(covered.insert(mark), "`{mark}` is listed twice");
        }
    }
    let missing: Vec<&&str> = all.iter().filter(|mark| !covered.contains(*mark)).collect();
    assert!(
        missing.is_empty(),
        "these marks are neither witnessed nor recorded: {missing:?}"
    );
}

#[test]
fn a_mark_recorded_unrendered_is_not_printed_by_a_corpus_render() {
    // The ledger's other direction: a mark recorded as one no corpus document can raise must not be
    // raised by one. Without this, a document added to the corpus could start exercising a mark the
    // ledger still calls unexercised.
    let directory = scratch();
    let marks = &vocabulary().marks;
    let mut unrecognisable: std::collections::BTreeSet<&str> = std::collections::BTreeSet::new();
    for document in documents::reader_documents()
        .into_iter()
        .chain(documents::writer_documents())
        .chain(documents::render_documents())
    {
        if extension(document.name).is_none() {
            continue;
        }
        let strings = render_strings(&directory, document.name);
        for entry in coverage::UNRENDERED {
            for mark in entry.marks {
                let Some(pattern) = sentence(marks.get(mark)) else {
                    unrecognisable.insert(mark);
                    continue;
                };
                for line in &strings {
                    assert!(
                        !pattern.is_match(line),
                        "`{mark}` is recorded unrendered, but `{}` prints {line:?}",
                        document.name
                    );
                }
            }
        }
    }
    assert_eq!(
        unrecognisable,
        UNRECOGNISABLE.iter().copied().collect(),
        "the marks this check cannot recognise changed: say so in `UNRECOGNISABLE` rather than \
         skipping them"
    );
}

/// The marks [`sentence`] cannot recognise among those recorded unrendered: each names a value rather
/// than a sentence. Listing them keeps the blind spot stated instead of silent — a short mark added to
/// the ledger must be added here.
const UNRECOGNISABLE: [&str; 2] = ["xlsx_boolean_false", "xlsx_boolean_true"];

/// Every line one corpus document's render prints: its report, its notes, each page's report, or
/// the reason it was refused.
fn render_strings(directory: &tempfile::TempDir, name: &str) -> Vec<String> {
    let (mut reports, notes, sources) = render_parts(directory, name);
    reports.extend(notes);
    reports.extend(sources);
    reports
}

/// The same, kept apart: the lines a report of what was not drawn holds — the document's and every
/// page's — the document's notes, and what each page is a picture of.
fn render_parts(
    directory: &tempfile::TempDir,
    name: &str,
) -> (Vec<String>, Vec<String>, Vec<String>) {
    let document =
        documents::document(name).unwrap_or_else(|| panic!("`{name}` is not a corpus document"));
    let path = write(directory.path(), name, &(document.build)());
    let (mut reports, mut notes, mut sources) = (Vec::new(), Vec::new(), Vec::new());
    match render(&path, name) {
        Rendered::Pages {
            pages,
            unshown,
            notes: document_notes,
            ..
        } => {
            reports.extend(unshown);
            notes.extend(document_notes);
            for page in pages {
                reports.extend(page.unshown);
                // What the picture is a picture of is a printed sentence too.
                sources.extend(page.source);
            }
        }
        Rendered::Refused(failure) => reports.push(failure.reason),
    }
    (reports, notes, sources)
}

/// The marks that state what a page *drew* where the document stated it differently — a run at a
/// smaller size, a page's own margins, a placeholder drawn where its kind puts it, an anchor resolved
/// to the text's own place, the face the pages are drawn in. They are notes of the document's, never
/// losses: the report of what was not drawn holds losses only, and its name is its contract. The list
/// is kept by hand, so the test below also checks the other direction — every line a corpus render
/// puts in its notes is one of these — and a new mark of this kind therefore cannot slip in unlisted.
const NOTES: [&str; 13] = [
    "render_anchor_unresolved",
    "render_columns",
    "render_face_default",
    "render_face_substituted",
    "render_justified",
    "render_margins_refused",
    "render_page_size_ignored",
    "render_paper_unknown",
    "render_placeholder_default",
    "render_sections",
    "render_size_clamped",
    "render_slide_size_ignored",
    "render_slide_size_partial",
];

#[test]
fn a_line_that_states_what_was_drawn_is_a_note_not_a_loss() {
    // Every mark that says something was drawn is a note, and no report of what was not drawn holds
    // one. Checked over the whole corpus, so a mark moved into a report again is caught here rather
    // than by a reader who trusted the report's name. The other direction is checked in the same
    // pass: every line a corpus render puts in its notes is one of these marks, so a note mark added
    // to an arm cannot stay unlisted.
    let directory = scratch();
    let marks = &vocabulary().marks;
    let patterns: Vec<(&str, regex::Regex)> = NOTES
        .iter()
        .map(|mark| {
            (
                *mark,
                sentence(marks.get(mark)).expect("a note's own sentence"),
            )
        })
        .collect();
    let mut stated = 0_usize;
    for document in documents::reader_documents()
        .into_iter()
        .chain(documents::writer_documents())
        .chain(documents::render_documents())
    {
        if extension(document.name).is_none() {
            continue;
        }
        let (reports, notes, _) = render_parts(&directory, document.name);
        for line in &reports {
            for (mark, pattern) in &patterns {
                assert!(
                    !pattern.is_match(line),
                    "`{mark}` states what was drawn, but `{}` names it as a loss: {line:?}",
                    document.name
                );
            }
        }
        for line in &notes {
            assert!(
                patterns.iter().any(|(_, pattern)| pattern.is_match(line)),
                "`{}` states {line:?} in its notes, which is not one of the note marks above",
                document.name
            );
            stated += 1;
        }
    }
    assert!(stated > 0, "a corpus document states a note at all");
}

#[test]
fn every_witnessed_note_is_printed_in_the_notes() {
    // The ledger's own direction: a note mark a corpus document witnesses is printed in that
    // document's notes, so the list above is not a list of marks nothing raises.
    let directory = scratch();
    let mut witnessed = 0_usize;
    for entry in coverage::RENDER_WITNESSES {
        if !NOTES.contains(&entry.mark) {
            continue;
        }
        let (_, notes, _) = render_parts(&directory, entry.document);
        assert!(
            notes.iter().any(|line| line.contains(entry.witness)),
            "`{}`: {} does not print {:?} in its notes {notes:?}",
            entry.mark,
            entry.document,
            entry.witness
        );
        witnessed += 1;
    }
    assert!(witnessed > 0, "the notes are witnessed at all");
}

// ── PDF fixtures ──────────────────────────────────────────────────────────────────────────────

/// A minimal PDF with `count` pages of the stated size in points.
fn pdf_with_pages(count: usize, width: f32, height: f32) -> Vec<u8> {
    use lopdf::{Dictionary, Document, Object};

    let mut document = Document::with_version("1.7");
    let pages = document.new_object_id();
    let mut kids = Vec::with_capacity(count);
    for _ in 0..count {
        let page = document.add_object(Object::Dictionary(Dictionary::from_iter([
            ("Type", Object::Name(b"Page".to_vec())),
            ("Parent", Object::Reference(pages)),
            (
                "MediaBox",
                Object::Array(vec![
                    Object::Real(0.0),
                    Object::Real(0.0),
                    Object::Real(width),
                    Object::Real(height),
                ]),
            ),
        ])));
        kids.push(Object::Reference(page));
    }
    document.objects.insert(
        pages,
        Object::Dictionary(Dictionary::from_iter([
            ("Type", Object::Name(b"Pages".to_vec())),
            ("Kids", Object::Array(kids)),
            (
                "Count",
                Object::Integer(i64::try_from(count).expect("a page count")),
            ),
        ])),
    );
    let catalog = document.add_object(Object::Dictionary(Dictionary::from_iter([
        ("Type", Object::Name(b"Catalog".to_vec())),
        ("Pages", Object::Reference(pages)),
    ])));
    document.trailer.set("Root", Object::Reference(catalog));
    let mut bytes = Vec::new();
    document.save_to(&mut bytes).expect("a written PDF");
    bytes
}

/// A one-sheet workbook whose worksheet part is `sheet`, for a hand-built document.
fn xlsx_workbook(sheet: &str) -> Vec<u8> {
    let workbook = concat!(
        r#"<workbook xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" "#,
        r#"xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships">"#,
        r#"<sheets><sheet name="Data" sheetId="1" r:id="rId1"/></sheets></workbook>"#,
    );
    let relationships = concat!(
        r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">"#,
        r#"<Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet" "#,
        r#"Target="worksheets/sheet1.xml"/></Relationships>"#,
    );
    zip_parts(&[
        ("xl/workbook.xml", workbook.as_bytes()),
        ("xl/_rels/workbook.xml.rels", relationships.as_bytes()),
        ("xl/worksheets/sheet1.xml", sheet.as_bytes()),
    ])
}

/// A ZIP package from the named parts, for a hand-built document.
fn zip_parts(parts: &[(&str, &[u8])]) -> Vec<u8> {
    use std::io::Write as _;
    let mut buffer = std::io::Cursor::new(Vec::new());
    {
        let mut zip = zip::ZipWriter::new(&mut buffer);
        for (name, bytes) in parts {
            zip.start_file(*name, zip::write::SimpleFileOptions::default())
                .expect("a part");
            zip.write_all(bytes).expect("part bytes");
        }
        zip.finish().expect("a package");
    }
    buffer.into_inner()
}

// ── the honesty of the report ────────────────────────────────────────────────────────────────

/// The marks a package part owns: the document's report names each once, from the parts themselves,
/// and no page ever names one. An object is a part the document states, never a frame's content, so
/// no page names an object either.
///
/// Only the three Office families are listed: this test skips every other file, and a PDF's own
/// losses — an attached file, a media annotation — are witnessed where a PDF is rendered, by
/// `every_rendering_mark_is_witnessed_or_recorded_unexercised` over the corpus.
const DOCUMENT_KINDS: [&str; 8] = [
    "render_unshown_chart",
    "render_unshown_chart_sheet",
    "render_unshown_diagram",
    "render_unshown_drawing",
    "render_unshown_object",
    "render_unshown_media",
    "render_unshown_video",
    "render_unshown_audio",
];

/// The marks content on a page owns: a page names each, and the document's report never does.
const PAGE_KINDS: [&str; 8] = [
    "render_unshown_picture",
    "render_unshown_missing_part",
    "render_unshown_equation",
    "render_unshown_shape",
    "render_unshown_link",
    "render_unshown_crop",
    "render_unshown_formula",
    "render_unshown_rendered_page",
];

/// Every word a document's reading gives as page text is drawn on a page or named in the report.
///
/// The words are the reading's own: the reading is the engine's statement of what a document holds,
/// so a construct the renderer drops in silence shows up here as a word no page draws and no line of
/// the report names. A reading also describes what is not page text — a cell's address, a link's
/// target, a sheet's name, the block it opens for a chart's cached numbers or a slide's speaker notes
/// — and [`content_words`] removes each of those descriptions before the words are compared.
///
/// Four cases the rule does not reach are stated where they are skipped: a file whose content is not
/// a document this renderer draws, one whose reading is refused, one whose reading states no text,
/// and one whose pages pass the render's bound (the pages past it are named, and the words on them
/// are that sentence's). The anti-vacuity floor below is counted after those skips, so it cannot pass
/// while nothing was compared.
///
/// The page's report and the document's are read as two lists, and each kind is checked at the level
/// that owns it, so one part named twice — on its page and in the document's report — fails. The
/// document's list also carries the answer's notes, which name no part: they are merged in because a
/// note's words are words the render stated, and the check above is about which level names a loss.
///
/// The limit: a part named twice *within* one report is not caught, because the two lists are
/// compared by the kind each names rather than by the part. The kinds that could repeat inside one
/// report — an object and a drawing at document level — have their own witnesses instead.
#[test]
fn every_word_a_document_holds_is_drawn_or_named() {
    let directory = scratch();
    let mut checked = 0_usize;
    let mut families = std::collections::BTreeSet::new();
    for document in documents::reader_documents()
        .into_iter()
        .chain(documents::writer_documents())
        .chain(documents::render_documents())
    {
        let Some(extension) = extension(document.name) else {
            continue;
        };
        if !matches!(
            extension,
            "docx" | "docm" | "pptx" | "pptm" | "xlsx" | "xlsm"
        ) {
            continue;
        }
        let path = write(directory.path(), document.name, &(document.build)());
        // The rule is stated over the three Office families, whose pages carry the lines this test
        // compares (a PDF's pages are the outside comparison's): a file whose content is not one of
        // them — a plain-text file named `.docx` — is not a document that fails to draw its pages.
        if !matches!(
            reading::detect(&path, document.name),
            Ok(reading::Kind::Pdf
                | reading::Kind::Docx
                | reading::Kind::Xlsx
                | reading::Kind::Pptx)
        ) {
            continue;
        }
        let out = directory.path().join(format!("{}-read", document.name));
        std::fs::create_dir_all(&out).expect("an output folder");
        let Reading::Text { text, .. } = reading::read(&path, document.name, &out) else {
            continue;
        };
        let Rendered::Pages {
            pages,
            unshown,
            notes,
            ..
        } = render(&path, document.name)
        else {
            let held = content_words(&text, &std::collections::BTreeSet::new());
            if held.is_empty() {
                continue;
            }
            panic!(
                "{}: the reading gives {held:?} and the render refused the document",
                document.name
            );
        };
        // The two reports are read as two lists: a kind one level owns must never be named by the
        // other, so a part the document report counted cannot also be named on a page.
        let page_report: Vec<String> = pages
            .iter()
            .flat_map(|page| page.unshown.iter().cloned())
            .collect();
        let document_report: Vec<String> = unshown.iter().chain(notes.iter()).cloned().collect();
        let marks = &vocabulary().marks;
        for mark in DOCUMENT_KINDS {
            let pattern = sentence(marks.get(mark)).expect("a kind's own sentence");
            for line in &page_report {
                assert!(
                    !pattern.is_match(line),
                    "{}: `{mark}` is the document's loss, but a page names {line:?}",
                    document.name
                );
            }
        }
        for mark in PAGE_KINDS {
            let pattern = sentence(marks.get(mark)).expect("a kind's own sentence");
            for line in &document_report {
                assert!(
                    !pattern.is_match(line),
                    "{}: `{mark}` is a page's loss, but the document names {line:?}",
                    document.name
                );
            }
        }
        let truncated = vocabulary().marks.format(
            "render_pages_truncated",
            &[("max", &vocabulary().render.pages_max.to_string())],
        );
        let reported: std::collections::BTreeSet<String> = page_report
            .iter()
            .chain(document_report.iter())
            .flat_map(|line| tokens(line))
            .collect();
        let held = content_words(&text, &reported);
        if held.is_empty() {
            continue;
        }
        if document_report.contains(&truncated) {
            continue;
        }
        checked += held.len();
        families.insert(extension.to_string());
        let drawn: std::collections::BTreeSet<String> = pages
            .iter()
            .flat_map(|page| page.lines.iter())
            .flat_map(|line| tokens(&line.text))
            .chain(reported.iter().cloned())
            .collect();
        let missing: Vec<&String> = held.iter().filter(|word| !drawn.contains(*word)).collect();
        assert!(
            missing.is_empty(),
            "{}: the reading gives {missing:?}, which no page draws and no line of either report \
             names; the page report is {page_report:?} and the document report is \
             {document_report:?}",
            document.name,
        );
    }
    assert!(
        checked > 100,
        "the readings' own text is read ({checked} words)"
    );
    for family in ["docx", "pptx", "xlsx"] {
        assert!(
            families.contains(family),
            "the rule covers every Office family ({families:?})"
        );
    }
}

/// A document's extension, or `None` for a name that states none.
fn extension(name: &str) -> Option<&str> {
    name.rsplit_once('.').map(|(_, extension)| extension)
}

/// The words a reading gives as page text, lowercased: its own descriptions removed.
///
/// A reading writes three kinds of line. A **label** is a line one of its marks states — `Sheet
/// "First":`, `Table 1: 2 rows, 2 columns`, `Footnote 1:` — and is dropped, because it names what
/// follows rather than states page text. A **described block** is a label and the indented lines
/// under it, for something no page shows at all: a slide's speaker notes, a document's comments, a
/// chart's cached numbers, a workbook's defined names, a note the text does not reference. The whole
/// block is dropped. Everything else is **content**, and its words are page text.
///
/// A content line carries remarks of the reading's own in brackets and parentheses — a cell's
/// address, a table's coordinates, a link's target and display text, a merged range, a formula, a
/// quoted name — and each is removed with the span that states it.
fn content_words(text: &str, reported: &std::collections::BTreeSet<String>) -> Vec<String> {
    let cleaned = cleaned(text);
    let lines: Vec<&str> = cleaned.lines().collect();
    let mut words = Vec::new();
    // The words this document's own labels state, and only those: a word the reading prints as a
    // label here is not page text here, and a word it prints as content elsewhere in the corpus is
    // still content here.
    let mut excused = std::collections::BTreeSet::new();
    // The cells the workbook hides: the renderer is required not to draw them, so their values are
    // not page text either.
    let hidden = hidden_cells(&lines);
    let mut described: Option<usize> = None;
    let mut index = 0;
    while index < lines.len() {
        let line = lines[index];
        let depth = indentation(line);
        let trimmed = line.trim();
        index += 1;
        if trimmed.is_empty() {
            continue;
        }
        if let Some(level) = described {
            // A described block runs until a line stands outside it, and everything in it is the
            // reading's description of something no page shows.
            if depth > level {
                excused.extend(tokens(trimmed));
                continue;
            }
            described = None;
        }
        if describes(trimmed) || named(trimmed, reported) {
            described = Some(depth);
            excused.extend(tokens(trimmed));
            continue;
        }
        let header = lines
            .get(index)
            .is_some_and(|next| indentation(next) > depth && !next.trim().is_empty());
        if header || label(trimmed) {
            excused.extend(tokens(trimmed));
            continue;
        }
        words.extend(line_words(trimmed, &hidden));
    }
    words.retain(|word| !excused.contains(word));
    words
}

/// Whether a label names a construct the render's report itself names: every word of the label but
/// its number stands in a line of the report, so the block it opens is the one the report excuses —
/// a slide the deck hides or does not list, a sheet it hides, a page it could not produce.
fn named(label: &str, reported: &std::collections::BTreeSet<String>) -> bool {
    let words = tokens(label);
    words.len() > 1
        && words
            .iter()
            .filter(|word| !word.chars().all(|character| character.is_ascii_digit()))
            .all(|word| reported.contains(word))
}

/// How far a line is indented: the reading indents what a label introduces.
fn indentation(line: &str) -> usize {
    line.chars()
        .take_while(|character| character.is_whitespace())
        .count()
}

/// Whether a label introduces a block that is not page text: a slide's speaker notes, a document's
/// comments, a chart's cached numbers, a slide or sheet the document hides, a note the text does not
/// reference. Each is something a reading describes and no page shows, and the reading says so in
/// its own words.
fn describes(line: &str) -> bool {
    let starts = |prefix: &str| line.starts_with(prefix);
    let opens = |prefix: &str, ending: &str| starts(prefix) && line.ends_with(ending);
    // A deck's speaker notes and comments, a slide or a sheet the document hides, a chart's cached
    // numbers, a workbook's defined names, a footnote the text does not reference.
    // A cell's comment is the reading's description of something no page draws, and it is led by
    // the cell it stands on: `A2 comment: ...`.
    let commented = body(line).starts_with("comment");
    commented
        || opens("Slide ", " notes:")
        || opens("Slide ", " comments:")
        || opens("Slide ", "(hidden slide)")
        || opens("Sheet \"", "(hidden):")
        || opens("Sheet \"", "(very hidden):")
        || opens("Sheet \"", "(no values)")
        || starts("Chart:")
        || starts("Chart (")
        || starts("Defined names:")
        || starts("(diagram text")
        || (starts("Comment ") && line.ends_with(':'))
        || line.contains("(not referenced in the text):")
}

/// A line with the cell address that leads it removed: `A2 comment: no` is `comment: no`.
fn body(line: &str) -> &str {
    let line = line.trim();
    let end = line
        .find(|character: char| !character.is_ascii_alphanumeric() && character != '$')
        .unwrap_or(line.len());
    if end == 0 || !address(&line[..end]) {
        return line;
    }
    line[end..].trim_start_matches([':', ' ']).trim_start()
}

/// Whether a line is one of the reading's own labels: a mark's text with its placeholders filled,
/// matched by the words the mark states before its first placeholder and after its last.
fn label(line: &str) -> bool {
    label_shapes().iter().any(|(prefix, suffix)| {
        // A mark that is nothing but placeholders states no fixed words, so it cannot name a line.
        if prefix.len() + suffix.len() < 2 || !line.starts_with(prefix.as_str()) {
            return false;
        }
        let rest = &line[prefix.len()..];
        if suffix.is_empty() {
            rest.is_empty()
        } else {
            rest.contains(suffix.as_str())
        }
    })
}

/// The shape of every mark the reading prints: its text up to its first placeholder, and from its
/// last. A shape with neither is the mark's whole text.
fn label_shapes() -> &'static [(String, String)] {
    static SHAPES: std::sync::OnceLock<Vec<(String, String)>> = std::sync::OnceLock::new();
    SHAPES.get_or_init(|| {
        let marks = &vocabulary().marks;
        let mut shapes: Vec<(String, String)> = marks
            .printed(Producer::Reading)
            .map(|(_, text)| shape(text))
            .collect();
        for label in [
            &document_engine::vocabulary::ppt_slide_labels().slide,
            &document_engine::vocabulary::ppt_slide_labels().notes,
            &document_engine::vocabulary::ppt_slide_labels().comments,
        ] {
            shapes.push(shape(label));
        }
        shapes
    })
}

/// One mark's text as the two ends a line must match, its placeholders standing between them.
fn shape(text: &str) -> (String, String) {
    let prefix = text.split('{').next().unwrap_or_default().to_string();
    let suffix = match text.rsplit_once('}') {
        Some((_, rest)) => rest.to_string(),
        None => text.to_string(),
    };
    (prefix, suffix)
}

/// The reading's text with its bracketed marks and the spans they name removed: a deletion and a
/// move's source are marked rather than shown, and every other bracket is a mark of its own.
fn cleaned(text: &str) -> String {
    let mut text = text.to_string();
    for marker in ["del", "moved-away"] {
        let open = format!("[{marker}]");
        let close = format!("[/{marker}]");
        while let Some(start) = text.find(&open) {
            let Some(end) = text[start..].find(&close) else {
                break;
            };
            text.replace_range(start..start + end + close.len(), "");
        }
    }
    let mut out = String::with_capacity(text.len());
    let mut depth = 0_usize;
    for character in text.chars() {
        match character {
            '[' => depth += 1,
            ']' => depth = depth.saturating_sub(1),
            _ if depth == 0 => out.push(character),
            _ => {}
        }
    }
    out
}

/// One content line's words: its cell's address, its formula's text and its quoted names removed,
/// and a cell the workbook hides left out — the renderer names it instead of drawing it.
fn line_words(
    line: &str,
    hidden: &(
        std::collections::BTreeSet<u32>,
        std::collections::BTreeSet<u32>,
    ),
) -> Vec<String> {
    let mut words = Vec::new();
    for part in line.split('|') {
        // A cell the reading states together with a link is the link's: what it prints beside the
        // address is the link's display text, not a value a page shows. A cell a merged range covers
        // is drawn once, by the range's anchor.
        let lower = part.to_lowercase();
        if lower.contains("link:") || lower.contains("in merged range") {
            continue;
        }
        // A parenthesised span is the reading's own remark — a merge's range, a formula's text, a
        // cell's state — and a quoted one is a name it states.
        let part = unquoted(part.trim());
        let mut part = part.as_str();
        // A cell's address and a table's coordinates stand before the value they name, each ending
        // with a colon: `A1:`, `R2: C3:`.
        let mut hidden_cell = false;
        while let Some((head, rest)) = part.split_once(':') {
            if !address(head.trim()) {
                break;
            }
            hidden_cell |= hidden.0.contains(&column_of(head.trim()))
                || hidden.1.contains(&row_of(head.trim()));
            part = rest.trim();
        }
        if hidden_cell {
            continue;
        }
        words.extend(tokens(part));
    }
    words
}

/// The cells a reading says the workbook hides, by column and by row, from the two marks that name
/// the ranges: `(hidden columns: C:D)` and `(hidden rows: 1-2)`.
fn hidden_cells(
    lines: &[&str],
) -> (
    std::collections::BTreeSet<u32>,
    std::collections::BTreeSet<u32>,
) {
    let mut columns = std::collections::BTreeSet::new();
    let mut rows = std::collections::BTreeSet::new();
    for line in lines {
        let trimmed = line.trim();
        let stated = trimmed
            .strip_prefix("(hidden columns: ")
            .map(|rest| (rest, true))
            .or_else(|| {
                trimmed
                    .strip_prefix("(hidden rows: ")
                    .map(|rest| (rest, false))
            });
        let Some((ranges, is_columns)) = stated else {
            continue;
        };
        for range in ranges.trim_end_matches(')').split(',') {
            let (first, last) = range.split_once([':', '-']).unwrap_or((range, range));
            let (first, last) = if is_columns {
                (column_of(first), column_of(last))
            } else {
                (row_of(first), row_of(last))
            };
            for number in first..=last.max(first) {
                if is_columns {
                    columns.insert(number);
                } else {
                    rows.insert(number);
                }
            }
        }
    }
    (columns, rows)
}

/// The zero-based column an address or a column letter states.
fn column_of(text: &str) -> u32 {
    text.chars()
        .take_while(|character| character.is_ascii_alphabetic())
        .fold(0_u32, |value, character| {
            value * 26 + u32::from(character.to_ascii_uppercase() as u8 - b'A' + 1)
        })
        .saturating_sub(1)
}

/// The row an address or a row number states, or zero for a text that states none.
fn row_of(text: &str) -> u32 {
    text.trim_start_matches(|character: char| character.is_ascii_alphabetic() || character == '$')
        .parse()
        .unwrap_or_default()
}

/// A line with its parenthesised and quoted spans removed.
fn unquoted(line: &str) -> String {
    let mut out = String::with_capacity(line.len());
    let mut depth = 0_usize;
    let mut quoted = false;
    for character in line.chars() {
        match character {
            '"' => quoted = !quoted,
            '(' if !quoted => depth += 1,
            ')' if !quoted => depth = depth.saturating_sub(1),
            _ if depth == 0 && !quoted => out.push(character),
            _ => {}
        }
    }
    out
}

/// Whether a line's head is a cell's address or a table's coordinates: letters and digits only.
fn address(head: &str) -> bool {
    !head.is_empty()
        && head.split_whitespace().count() <= 2
        && head.split_whitespace().all(|token| {
            let letters = token.chars().take_while(|c| c.is_alphabetic()).count();
            letters > 0
                && letters <= 3
                && token[letters..]
                    .chars()
                    .all(|character| character.is_ascii_digit())
        })
}

/// The runs of letters and digits in a line, lowercased: a number a cell holds is a word, and so is
/// the value a formula computed. A date the reading spells in its own civil form is compared as the
/// cell it stands for, not as its digits — the renderer draws the format the cell states — so the
/// year of a date or a time on the line is left out, and a number anywhere else is not.
fn tokens(line: &str) -> Vec<String> {
    let mut words = Vec::new();
    let mut word = String::new();
    for character in line.chars() {
        if character.is_alphanumeric() {
            word.extend(character.to_lowercase());
        } else if !word.is_empty() {
            words.push(std::mem::take(&mut word));
        }
    }
    if !word.is_empty() {
        words.push(word);
    }
    if date_line(line) {
        return words.into_iter().filter(|word| !year(word)).collect();
    }
    words
}

/// Whether a line states a date or a time: two numbers joined by a separator.
fn date_line(line: &str) -> bool {
    let characters: Vec<char> = line.chars().collect();
    characters.windows(3).any(|window| {
        matches!(window[1], '/' | '-' | ':')
            && window[0].is_ascii_digit()
            && window[2].is_ascii_digit()
    })
}

/// Whether a word is a four-digit year.
fn year(word: &str) -> bool {
    word.len() == 4 && word.chars().all(|character| character.is_ascii_digit())
}

//! The text body a created slide's shape carries, transliterated from the pinned writer library.
//!
//! Only the shapes a created deck produces reach this: a text box whose runs state a size, a
//! weight and the library's default colour, with or without a bullet. The library's other options —
//! alignment, line spacing, indentation levels, spacing before and after, tab stops, hyperlinks,
//! outlining, glow, shadows, an explicit font — are never stated by the reference's content list, so
//! the branches that would write them are not carried across; the ones that decide the shape of the
//! text are.
//!
//! Three of the library's rules are load-bearing and are reproduced as they stand:
//!
//! - a line break in a run splits it into one paragraph per line, while a run whose text ends with
//!   one keeps the break inside its own `<a:t>` — the carriage return is written literally;
//! - a bullet makes its run the whole of a paragraph, so a list is one paragraph per item;
//! - the paragraph's closing `<a:endParaRPr>` states the *shape's* size, not the run's, and is
//!   written with that size whenever the shape states one.

use std::fmt::Write as _;

use super::super::super::js_round;
use super::super::super::text::Markup;
use super::super::super::xml;
use super::super::draw::FONT_COLOR;
use super::super::{HUNDREDTHS, SLIDE_LANG};

/// The margin a bullet's text starts at: the library's own 27 points, in EMU, and the negative
/// indent that pulls the bullet itself into it.
const BULLET_MARGIN: &str = "342900";
const BULLET_INDENT: &str = "-342900";

/// The bullet character the library draws for an unnamed bullet, as the entity reference it writes.
const BULLET_CHAR: &str = "&#x2022;";

/// The font, numbering style and first number a numbered bullet states.
const BULLET_FONT: &str = "+mj-lt";
const BULLET_STYLE: &str = "arabicPeriod";
const BULLET_START: &str = "1";

/// The kind of bullet a run carries.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Bullet {
    /// No bullet: the paragraph states an empty margin and `<a:buNone/>`.
    None,
    /// The library's default character.
    Char,
    /// Numbered.
    Number,
}

/// One run of a text body: its text and its bullet.
#[derive(Clone, Debug)]
pub(super) struct Run {
    pub(super) text: String,
    pub(super) bullet: Bullet,
}

impl Run {
    /// One run stating `text`, with `bullet`.
    pub(super) fn new(text: &str, bullet: Bullet) -> Self {
        Self {
            text: text.to_string(),
            bullet,
        }
    }
}

/// The options a text shape carries that its runs inherit: the size and the weight. A created deck
/// states no other option a run's properties would write, and every run inherits these, so a run
/// states its shape's.
#[derive(Clone, Copy, Debug)]
pub(super) struct TextOptions {
    /// The size in points, which the library writes in hundredths.
    pub(super) font_size: f64,
    pub(super) bold: bool,
}

/// The text body of one shape: its options and the runs the block states.
#[derive(Clone, Debug)]
pub(super) struct TextBody {
    pub(super) options: TextOptions,
    pub(super) runs: Vec<Run>,
}

/// A size in hundredths of a point, as the library rounds it.
#[expect(
    clippy::cast_possible_truncation,
    reason = "a text size is far below 2^53 hundredths"
)]
fn size_hundredths(points: f64) -> i64 {
    js_round(points * HUNDREDTHS) as i64
}

/// `text` with every line break normalised to a carriage return and a line feed, as the library
/// normalises both a run's text and a slide's notes — its own `/\r*\n/g`, so a run of carriage
/// returns before a line feed is one break. A carriage return with no line feed after it is left as
/// it is.
pub(super) fn crlf(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut characters = text.chars().peekable();
    while let Some(character) = characters.next() {
        match character {
            '\r' => {
                let mut run = 1;
                while characters.peek() == Some(&'\r') {
                    characters.next();
                    run += 1;
                }
                if characters.peek() == Some(&'\n') {
                    characters.next();
                    out.push_str("\r\n");
                } else {
                    for _ in 0..run {
                        out.push('\r');
                    }
                }
            }
            '\n' => out.push_str("\r\n"),
            _ => out.push(character),
        }
    }
    out
}

/// The runs of `body` as the paragraphs the library writes: one per run, after a run whose text
/// holds a line break it does not end with is split into one run per line. The library's own
/// grouping loop is not reproduced because it cannot do anything else for the bodies this arm
/// builds — a block is one run, and a list's runs are all bulleted, each of which closes itself.
fn paragraphs(body: &TextBody) -> Vec<Vec<Run>> {
    let mut paragraphs = Vec::with_capacity(body.runs.len());
    for run in &body.runs {
        let text = crlf(&run.text);
        if text.contains("\r\n") && !text.ends_with('\n') {
            for line in text.split("\r\n") {
                paragraphs.push(vec![Run {
                    text: line.to_string(),
                    bullet: run.bullet,
                }]);
            }
        } else {
            paragraphs.push(vec![Run {
                text,
                bullet: run.bullet,
            }]);
        }
    }
    paragraphs
}

/// The paragraph properties one run states.
fn paragraph_properties(bullet: Bullet) -> String {
    match bullet {
        Bullet::None => "<a:pPr indent=\"0\" marL=\"0\"><a:buNone/></a:pPr>".to_string(),
        Bullet::Char => format!(
            "<a:pPr marL=\"{BULLET_MARGIN}\" indent=\"{BULLET_INDENT}\"><a:buSzPct val=\"100000\"/><a:buChar char=\"{BULLET_CHAR}\"/></a:pPr>"
        ),
        Bullet::Number => format!(
            "<a:pPr marL=\"{BULLET_MARGIN}\" indent=\"{BULLET_INDENT}\"><a:buSzPct val=\"100000\"/><a:buFont typeface=\"{BULLET_FONT}\"/><a:buAutoNum type=\"{BULLET_STYLE}\" startAt=\"{BULLET_START}\"/></a:pPr>"
        ),
    }
}

/// The properties a run states: the language, the size, the weight and the colour, in the
/// library's own order. A run with no text is not written at all, which is why this is only called
/// for a run that has some.
fn run_properties(options: TextOptions) -> String {
    let mut properties = format!("<a:rPr lang=\"{SLIDE_LANG}\"");
    write!(properties, " sz=\"{}\"", size_hundredths(options.font_size))
        .expect("writing to a string cannot fail");
    if options.bold {
        properties.push_str(" b=\"1\"");
    }
    properties.push_str(" dirty=\"0\">");
    properties.push_str(&colour_element(FONT_COLOR));
    properties.push_str("</a:rPr>");
    properties
}

/// The colour selection a run states, in the library's own shape.
fn colour_element(colour: &str) -> String {
    format!("<a:solidFill><a:srgbClr val=\"{colour}\"/></a:solidFill>")
}

/// The closing paragraph mark: the shape's size, which every shape a created deck writes states.
fn end_paragraph(options: TextOptions) -> String {
    format!(
        "<a:endParaRPr lang=\"{SLIDE_LANG}\" sz=\"{}\" dirty=\"0\"/>",
        size_hundredths(options.font_size)
    )
}

/// One body's paragraphs, shared by a shape's `<p:txBody>` and a cell's `<a:txBody>`: the body
/// properties and list style the caller states, then one paragraph per line.
fn body_paragraphs(prefix: &str, body: &TextBody) -> String {
    let paragraphs = paragraphs(body);
    let mut xml_text = prefix.to_string();
    for paragraph in &paragraphs {
        xml_text.push_str("<a:p>");
        for run in paragraph {
            xml_text.push_str(&paragraph_properties(run.bullet));
            if !run.text.is_empty() {
                write!(
                    xml_text,
                    "<a:r>{}{}</a:r>",
                    run_properties(body.options),
                    Markup::Drawing.text_element(&xml::xml_escape(&run.text)),
                )
                .expect("writing to a string cannot fail");
            }
        }
        xml_text.push_str(&end_paragraph(body.options));
        xml_text.push_str("</a:p>");
    }
    xml_text
}

/// A whole `<p:txBody>`: the library's own body properties for a text box, and its paragraphs.
pub(super) fn text_body(body: &TextBody) -> String {
    let mut xml_text = body_paragraphs(
        "<p:txBody><a:bodyPr wrap=\"square\" rtlCol=\"0\" anchor=\"ctr\"></a:bodyPr><a:lstStyle/>",
        body,
    );
    xml_text.push_str("</p:txBody>");
    xml_text
}

/// A table cell's `<a:txBody>`: the same text machinery under a cell's own body properties, the
/// cell's size closing each paragraph. A heading cell states its weight on the run.
pub(super) fn cell_body(text: &str, points: f64, bold: bool) -> String {
    let body = TextBody {
        options: TextOptions {
            font_size: points,
            bold,
        },
        runs: vec![Run::new(text, Bullet::None)],
    };
    let mut xml_text = body_paragraphs("<a:txBody><a:bodyPr/><a:lstStyle/>", &body);
    xml_text.push_str("</a:txBody>");
    xml_text
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A body of one run at `font_size`.
    fn body(text: &str, bullet: Bullet) -> TextBody {
        TextBody {
            options: TextOptions {
                font_size: 16.0,
                bold: false,
            },
            runs: vec![Run::new(text, bullet)],
        }
    }

    #[test]
    fn a_line_break_a_run_ends_with_stays_inside_its_own_text() {
        // The library splits only a run whose break it does not end with, so a trailing one is
        // written into the text element as a carriage return and a line feed.
        let xml_text = text_body(&body("a\n", Bullet::None));
        assert_eq!(xml_text.matches("<a:p>").count(), 1, "{xml_text}");
        assert!(xml_text.contains("<a:t>a\r\n</a:t>"), "{xml_text}");
    }

    #[test]
    fn a_run_of_carriage_returns_before_a_line_feed_is_one_break() {
        // The library's `/\r*\n/g` swallows every carriage return before the line feed, so a run of
        // them is one break however long it is; a run with no line feed after it is untouched.
        assert_eq!(crlf("a\r\r\nb"), "a\r\nb");
        assert_eq!(crlf("a\r\r\r\nb"), "a\r\nb");
        assert_eq!(crlf("a\r\rb"), "a\r\rb");
        assert_eq!(
            text_body(&body("a\r\r\nb", Bullet::None))
                .matches("<a:p>")
                .count(),
            2,
            "the collapsed break still splits the run into two paragraphs"
        );
    }

    #[test]
    fn a_run_with_no_text_is_not_written_and_the_paragraph_still_is() {
        let xml_text = text_body(&body("", Bullet::None));
        assert!(!xml_text.contains("<a:r>"), "{xml_text}");
        assert!(
            xml_text.contains("<a:endParaRPr lang=\"en-US\" sz=\"1600\" dirty=\"0\"/>"),
            "{xml_text}"
        );
    }

    #[test]
    fn a_heading_cell_states_its_weight_and_a_body_cell_does_not() {
        assert!(
            cell_body("A", 14.0, true)
                .contains("<a:rPr lang=\"en-US\" sz=\"1400\" b=\"1\" dirty=\"0\">"),
            "a heading cell is bold"
        );
        assert!(
            cell_body("A", 14.0, false).contains("<a:rPr lang=\"en-US\" sz=\"1400\" dirty=\"0\">"),
            "a body cell is not"
        );
    }
}

//! Laying a created document out as Word does, to find the page each bookmark starts on.
//!
//! The page is filled from the top, between the page's margins — or its header and footer where those
//! are taller than the margins leave — with each paragraph broken into lines at the column's own
//! width. Paragraphs break between lines; a page break starts a new page; a table's rows break across
//! pages between the lines of their cells, their columns sized to the content of those cells. The
//! footnotes of each page's lines take room at its bottom.
//!
//! # What a created document reaches
//!
//! One section, one column, inline pictures, tables whose rows break across pages between the lines
//! of their cells, footnotes at the page's bottom, a header and a footer, and the compatibility mode
//! the created document's settings state (Word 2013's, at which every older-mode rule is out of
//! reach, so East Asian breaking is on). Widow and orphan control, and a table row that may not split
//! across a page, are not carried: the created styles state no `w:widowControl` and the created
//! tables state no `w:cantSplit`, so neither rule can fire.
//!
//! A shape a created document does not write — a document in columns, a text box, a frame, an
//! equation, a floating drawing, a footnote that continues past its page — is not read by
//! [`read`], so the pass never sees one. The pass stops only where the measurer cannot read a
//! character or a table's cells' margins are wider than the room for the table.
//!
//! # The numbers
//!
//! Each pass lays the document out with the numbers the pass before worked out, so a table of
//! contents' entries are as wide as the numbers written into them make them. When the numbers stop
//! changing, they are the answer; when they are still changing after the reference's three passes,
//! the document is answered with no numbers at all, which leaves every field blank. A pass that stops
//! at something it cannot lay out keeps the numbers it placed before the stop — the reference does
//! the same — and leaves the page count unknown, which blanks the page-count token.

use std::collections::BTreeMap;

use super::breaks;
use super::read::{
    Block, Cell, Content, Field, Item, LineSpacing, Paragraph, ParagraphFormat, Table,
};
use super::text::{
    Font, measure_descent, measure_line_height, measure_text_height, measure_text_width,
    unknown_character, unknown_font,
};

/// The slack two lengths must differ by to count as different, from the layout constants data.
fn tolerance() -> f64 {
    super::constants::value("TOLERANCE").expect("the layout data states the tolerance")
}

/// The most passes the reference lays a document out for, from the layout constants data.
#[expect(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "the pass cap is a small whole number the data states"
)]
fn pass_cap() -> u32 {
    super::constants::value("PASSES").expect("the layout data states the pass cap") as u32
}

/// One line of a paragraph, as it is placed on a page.
struct Line {
    /// How tall the line is, in points, with the paragraph's own spacing: the space before is part of
    /// the paragraph's first line and the space after of its last, so a run of lines measures the
    /// same however it is cut — which is what lets a table row break across pages between its lines.
    height: f64,
    /// The page break the paragraph states before itself.
    page_break_before: bool,
}

/// The line a paragraph's text is broken into, as a token the fill places: a word, a space, a tab or
/// an inline picture.
enum Token {
    /// A maximal run of text between two break opportunities, in one or more fonts.
    Word(Vec<(String, Font)>),
    Space(Font),
    Tab(Font),
    /// An inline picture, by its own size in points.
    Picture {
        width: f64,
        height: f64,
    },
}

/// What a line has taken so far, while it is being filled.
struct Filling {
    /// How far across the line the fill has gone, trailing spaces included.
    position: f64,
    /// The widest ascent and deepest descent of the text on the line.
    ascent: f64,
    descent: f64,
    /// The tallest text on the line, and the tallest picture.
    tallest: f64,
    picture: f64,
    started: bool,
}

impl Filling {
    fn new() -> Self {
        Self {
            position: 0.0,
            ascent: 0.0,
            descent: 0.0,
            tallest: 0.0,
            picture: 0.0,
            started: false,
        }
    }
}

/// The state every measurement of one pass reads: the numbers the pass before worked out, how many
/// pages the document had, and whether the document's compatibility mode breaks lines between
/// Chinese and Japanese characters.
struct Pass<'a> {
    numbers: &'a BTreeMap<String, u32>,
    page_count: Option<u32>,
    ideographs: bool,
}

/// Lay the document out once. A stop in the header or the footer stops the layout before it places
/// anything, which is what the reference does.
#[expect(
    clippy::too_many_lines,
    reason = "the reference's own pagination loop, kept as one place the page advances"
)]
fn lay_out(content: &Content, pass: &Pass<'_>) -> super::Estimate {
    let page = content.page;
    let column = (page.width - page.left - page.right).max(1.0);
    let (Some(header), Some(footer)) = (
        blocks_height(&content.header, column, pass),
        blocks_height(&content.footer, column, pass),
    ) else {
        return super::Estimate::default();
    };
    // The body sits between the margins, or below a header taller than the margin leaves and above a
    // footer that reaches into it.
    let top = page.top.max(page.header + header);
    let bottom = page.height - page.bottom.max(page.footer + footer);
    let body = bottom - top;
    if body <= 0.0 {
        return super::Estimate::default();
    }
    let mut places: BTreeMap<String, u32> = BTreeMap::new();
    let mut page_index = 1;
    let mut used = 0.0;
    // The room the page's footnotes take at its bottom, which grows as notes are referenced.
    let mut note_area = 0.0;
    let mut count: u32 = 1;
    let mut stopped = false;
    let place = |places: &mut BTreeMap<String, u32>, bookmark: &Option<String>, page_index: u32| {
        if let Some(name) = bookmark {
            places.entry(name.clone()).or_insert(page_index);
        }
    };
    // Start a new page: nothing is on it and its footnotes take no room yet.
    macro_rules! new_page {
        () => {{
            count += 1;
            page_index += 1;
            used = 0.0;
            note_area = 0.0;
        }};
    }
    for block in &content.blocks {
        if stopped {
            break;
        }
        match block {
            Block::Paragraph(paragraph) => {
                // A page break of its own starts a new page, and the paragraph that carries it puts
                // nothing on either.
                if paragraph.items.len() == 1
                    && paragraph
                        .items
                        .iter()
                        .any(|item| matches!(item, Item::PageBreak))
                {
                    new_page!();
                    continue;
                }
                let Some(lines) = paragraph_lines(paragraph, column, pass) else {
                    stopped = true;
                    continue;
                };
                let note = match paragraph.footnote {
                    Some(index) => {
                        let Some(height) = content
                            .footnotes
                            .get(index)
                            .and_then(|note| note_height(note, column, pass))
                        else {
                            // A footnote the measurer cannot read stops the layout where its
                            // reference is placed, which keeps the numbers placed before that page —
                            // as the reference does — rather than losing every number in the document.
                            stopped = true;
                            break;
                        };
                        height
                    }
                    None => 0.0,
                };
                for (at, line) in lines.iter().enumerate() {
                    let needed = line.height;
                    if line.page_break_before && used > 0.0 {
                        new_page!();
                    }
                    // The footnote a paragraph refers to takes its room at the bottom of the page the
                    // paragraph starts on, so a line that only fits above it moves on with the note.
                    if at == 0
                        && note > 0.0
                        && used + needed + note > body - note_area
                        && used > 0.0
                    {
                        new_page!();
                    }
                    if used + needed > body - note_area && used > 0.0 {
                        new_page!();
                    }
                    if at == 0 {
                        place(&mut places, &paragraph.bookmark, page_index);
                        note_area += note;
                    }
                    used += needed;
                }
            }
            Block::Table(table) => {
                let Some(rows) = table_rows(table, column, pass) else {
                    stopped = true;
                    continue;
                };
                // A row taller than the room left on the page breaks across it, between the lines of
                // its cells: as many lines as fit stay on this page, and the rest go on from the top of
                // the next. The row is placed again on each page it reaches, which is what the
                // reference does and what makes the page count of a long cell come out.
                for row in &rows {
                    let mut rest: Vec<&[Line]> = row.cells.iter().map(Vec::as_slice).collect();
                    loop {
                        let room = body - note_area - used - row.break_border;
                        let mut take: Vec<usize> = rest
                            .iter()
                            .map(|lines| row.lines_that_fit(lines, room))
                            .collect();
                        if take.iter().all(|&count| count == 0)
                            && rest.iter().any(|lines| !lines.is_empty())
                        {
                            if used > 0.0 {
                                // No line of the row fits above the room left: start a page and try
                                // again, as the reference does.
                                new_page!();
                                continue;
                            }
                            // A row whose first line is taller than a whole page is placed rather
                            // than looped over, as a paragraph's own too-tall line is.
                            for (count, lines) in take.iter_mut().zip(&rest) {
                                if !lines.is_empty() {
                                    *count = 1;
                                }
                            }
                        }
                        let height = row.height_of(
                            rest.iter()
                                .zip(&take)
                                .map(|(lines, &count)| &lines[..count]),
                        );
                        used += height;
                        let left: Vec<&[Line]> = rest
                            .iter()
                            .zip(&take)
                            .map(|(lines, &count)| &lines[count..])
                            .collect();
                        if left.iter().all(|lines| lines.is_empty()) {
                            break;
                        }
                        // The rest of the row goes on to the next page, where the table breaks.
                        rest = left;
                        new_page!();
                    }
                }
            }
        }
    }
    super::Estimate {
        bookmark_numbers: places,
        page_count: if stopped { None } else { Some(count) },
    }
}

/// How much room one footnote takes at the bottom of its page.
fn note_height(note: &Paragraph, column: f64, pass: &Pass<'_>) -> Option<f64> {
    let lines = paragraph_lines(note, column, pass)?;
    Some(lines_height(&lines))
}

/// The height a run of lines takes: each line's own height, its paragraph's spacing already counted.
fn lines_height(lines: &[Line]) -> f64 {
    lines.iter().map(|line| line.height).sum()
}

/// One row of a table as the page loop places it: its cells' lines, and the room its margins and
/// borders take.
struct RowLines {
    /// Each cell's lines, in the order the cells are read.
    cells: Vec<Vec<Line>>,
    /// The room the cells' own margins take above and below their text, summed: no reader tells the
    /// two apart.
    margins: f64,
    /// The border above the row and, on the last row, the table's bottom border. The reference counts
    /// no bottom border on any other row; the one a break draws below it is `break_border`.
    borders: f64,
    /// The border the table draws below a row it breaks: its own bottom border, repeated on every page
    /// a row part reaches. The last row carries that border in `borders` instead, so it has none.
    break_border: f64,
}

impl RowLines {
    /// The room the row's margins and borders take above and below its text.
    fn room(&self) -> f64 {
        self.margins + self.borders
    }

    /// How tall the row is when each cell gives the lines in `parts`: its tallest cell, with the room
    /// the row's margins and borders take.
    fn height_of<'a>(&self, parts: impl IntoIterator<Item = &'a [Line]>) -> f64 {
        let tallest = parts.into_iter().map(lines_height).fold(0.0_f64, f64::max);
        self.room() + tallest
    }

    /// How many of a cell's lines fit in `room`, from the first.
    fn lines_that_fit(&self, lines: &[Line], room: f64) -> usize {
        let mut height = self.room();
        let mut count = 0;
        for line in lines {
            if height + line.height > room + tolerance() {
                break;
            }
            height += line.height;
            count += 1;
        }
        count
    }
}

/// The height a stack of blocks takes, when it can be worked out at all.
fn blocks_height(blocks: &[Block], column: f64, pass: &Pass<'_>) -> Option<f64> {
    let mut height = 0.0;
    for block in blocks {
        match block {
            Block::Paragraph(paragraph) => {
                let lines = paragraph_lines(paragraph, column, pass)?;
                height += lines_height(&lines);
            }
            Block::Table(table) => {
                for row in table_rows(table, column, pass)? {
                    height += row.height_of(row.cells.iter().map(Vec::as_slice));
                }
            }
        }
    }
    Some(height)
}

/// A paragraph's lines, with the width the column leaves it.
fn paragraph_lines(paragraph: &Paragraph, column: f64, pass: &Pass<'_>) -> Option<Vec<Line>> {
    let format = &paragraph.format;
    let left = format.indent_left.max(0.0);
    let right = format.indent_right.max(0.0);
    let width = (column - left - right).max(1.0);
    let tokens = tokens_of(paragraph, pass)?;
    if tokens.is_empty() {
        // An empty paragraph is one empty line of its own default font.
        return Some(vec![empty_line(format)]);
    }
    let mut lines = Vec::new();
    let mut line = Filling::new();
    let mut available = width - format.first_line;
    for token in tokens {
        match token {
            Token::Space(font) => {
                line.position += measure_text_width(" ", &font, line.position);
                line.started = true;
            }
            Token::Tab(font) => {
                let next = next_tab(format, line.position);
                if next > available + tolerance() && line.started {
                    lines.push(finish_line(&line, format, lines.is_empty()));
                    line = Filling::new();
                    available = width;
                    line.position = next_tab(format, 0.0);
                } else {
                    line.position = next;
                }
                let descent = measure_descent(&font);
                let natural = measure_line_height(&font);
                line.descent = line.descent.max(descent);
                line.ascent = line.ascent.max(natural - descent);
                line.tallest = line.tallest.max(natural);
                line.started = true;
            }
            Token::Picture {
                width: picture,
                height,
            } => {
                if line.position + picture > available + tolerance() && line.started {
                    lines.push(finish_line(&line, format, lines.is_empty()));
                    line = Filling::new();
                    available = width;
                }
                line.position += picture;
                line.picture = line.picture.max(height);
                line.tallest = line.tallest.max(height);
                line.started = true;
            }
            Token::Word(pieces) => {
                let word_width = width_of(&pieces, line.position);
                if line.position + word_width > available + tolerance() && line.started {
                    lines.push(finish_line(&line, format, lines.is_empty()));
                    line = Filling::new();
                    available = width;
                }
                // A word wider than the whole line is cut character by character, as Word cuts it.
                if !line.started
                    && width_of(&pieces, line.position) > available - line.position + tolerance()
                {
                    // One buffer for a character's own spelling, reused across the cut: a cut is per
                    // character, and an allocation per character is what a long unbroken word pays
                    // for.
                    let mut character = String::new();
                    for (text, font) in &pieces {
                        for scalar in text.chars() {
                            character.clear();
                            character.push(scalar);
                            let advance = measure_text_width(&character, font, line.position);
                            if line.started && line.position + advance > available + tolerance() {
                                lines.push(finish_line(&line, format, lines.is_empty()));
                                line = Filling::new();
                                available = width;
                            }
                            line.position += measure_text_width(&character, font, line.position);
                            let descent = measure_descent(font);
                            let natural = measure_line_height(font);
                            line.descent = line.descent.max(descent);
                            line.ascent = line.ascent.max(natural - descent);
                            line.tallest = line.tallest.max(natural);
                            line.started = true;
                        }
                    }
                    continue;
                }
                line.position += width_of(&pieces, line.position);
                for (text, font) in &pieces {
                    let height = measure_text_height(text, font)
                        .unwrap_or_else(|| measure_line_height(font));
                    let descent = measure_descent(font);
                    line.descent = line.descent.max(descent);
                    line.ascent = line.ascent.max(height - descent);
                    line.tallest = line.tallest.max(height);
                }
                line.started = true;
            }
        }
    }
    if line.started || lines.is_empty() {
        lines.push(finish_line(&line, format, lines.is_empty()));
    }
    // The paragraph's space after belongs to its last line, so a cut row keeps the same total height
    // however its lines are shared between pages.
    if let Some(last) = lines.last_mut() {
        last.height += format.space_after.unwrap_or(0.0);
    }
    Some(lines)
}

/// One empty line of the paragraph's own default font.
fn empty_line(format: &ParagraphFormat) -> Line {
    let font = Font::default();
    Line {
        height: line_height(&font, format)
            + format.space_before.unwrap_or(0.0)
            + format.space_after.unwrap_or(0.0),
        page_break_before: format.page_break_before,
    }
}

/// One finished line, with the paragraph's own spacing before on the line that carries it. The space
/// after is added to the paragraph's last line once it is known (see [`paragraph_lines`]).
fn finish_line(line: &Filling, format: &ParagraphFormat, first: bool) -> Line {
    let natural = (line.ascent.max(line.picture) + line.descent).max(line.tallest);
    Line {
        height: spaced(natural, line.tallest, format)
            + if first {
                format.space_before.unwrap_or(0.0)
            } else {
                0.0
            },
        page_break_before: first && format.page_break_before,
    }
}

/// A line's height under the paragraph's own line spacing.
fn spaced(natural: f64, tallest: f64, format: &ParagraphFormat) -> f64 {
    match format.line_spacing {
        Some(LineSpacing::Multiple(multiple)) => natural + (multiple - 1.0) * tallest,
        Some(LineSpacing::Exact(points)) => points,
        Some(LineSpacing::AtLeast(points)) => natural.max(points),
        None => natural,
    }
}

/// The line height a paragraph's own spacing states.
fn line_height(font: &Font, format: &ParagraphFormat) -> f64 {
    let natural = measure_line_height(font);
    spaced(natural, natural, format)
}

/// The next tab stop after `position`, from the paragraph's own stops or the whole ones.
fn next_tab(format: &ParagraphFormat, position: f64) -> f64 {
    let slack = tolerance();
    let stated = format
        .tabs
        .iter()
        .find(|tab| **tab > position + slack)
        .copied();
    let stop = super::text::tab_stop();
    // A plain multiply and add, as the reference writes it: a fused operation would round once
    // where the reference rounds twice.
    stated.unwrap_or_else(|| ((position + slack) / stop).floor() * stop + stop)
}

/// A word's width, measured piece by piece from `start`.
fn width_of(pieces: &[(String, Font)], start: f64) -> f64 {
    let mut position = start;
    for (text, font) in pieces {
        position += measure_text_width(text, font, position);
    }
    position - start
}

/// Whether the measurer knows a run's face and every character of the text it measures in it.
fn known_text(text: &str, font: &Font) -> bool {
    text.is_empty() || (!unknown_font(font) && unknown_character(text, font).is_none())
}

/// A paragraph's items as the tokens the fill places, or `None` at a character or a face the measurer
/// does not know.
fn tokens_of(paragraph: &Paragraph, pass: &Pass<'_>) -> Option<Vec<Token>> {
    // One piece of the paragraph's text stream, in order: a character and the font it is drawn in, a
    // tab, or a picture.
    enum Piece {
        Character(char, usize),
        Tab(usize),
        Picture { width: f64, height: f64 },
    }
    let mut fonts: Vec<Font> = Vec::new();
    let font_of = |font: &Font, fonts: &mut Vec<Font>| -> usize {
        if let Some(at) = fonts.iter().position(|known| same_font(known, font)) {
            at
        } else {
            fonts.push(font.clone());
            fonts.len() - 1
        }
    };
    let mut pieces: Vec<Piece> = Vec::new();
    for item in &paragraph.items {
        match item {
            Item::Text(run) => {
                let east = super::text::east_asian_font(&run.font);
                let latin = font_of(&run.font, &mut fonts);
                let east = font_of(&east, &mut fonts);
                // The run's characters its own face measures, checked once rather than per character.
                let mut own = String::new();
                for character in run.text.chars() {
                    if breaks::is_east_asian(character) {
                        pieces.push(Piece::Character(character, east));
                    } else {
                        own.push(character);
                        pieces.push(Piece::Character(character, latin));
                    }
                }
                if !known_text(&own, &run.font) {
                    return None;
                }
            }
            Item::Tab => {
                let at = font_of(&Font::default(), &mut fonts);
                pieces.push(Piece::Tab(at));
            }
            Item::PageBreak => {}
            Item::Field(field) => {
                let text = field_text(field, pass);
                let at = font_of(&Font::default(), &mut fonts);
                for character in text.chars() {
                    pieces.push(Piece::Character(character, at));
                }
            }
            Item::Drawing { width, height } => pieces.push(Piece::Picture {
                width: *width,
                height: *height,
            }),
        }
    }
    let characters: Vec<char> = pieces
        .iter()
        .filter_map(|piece| match piece {
            Piece::Character(character, _) => Some(*character),
            _ => None,
        })
        .collect();
    let breaks = breaks::breaks(&characters, pass.ideographs);
    let mut tokens = Vec::new();
    let mut word: Vec<(String, Font)> = Vec::new();
    let mut at = 0;
    let flush = |word: &mut Vec<(String, Font)>, tokens: &mut Vec<Token>| {
        if !word.is_empty() {
            tokens.push(Token::Word(std::mem::take(word)));
        }
    };
    for piece in pieces {
        match piece {
            Piece::Character(character, font) => {
                if breaks.contains(&at) {
                    flush(&mut word, &mut tokens);
                }
                at += 1;
                if breaks::is_space(character) {
                    flush(&mut word, &mut tokens);
                    tokens.push(Token::Space(fonts[font].clone()));
                } else {
                    match word.last_mut() {
                        Some((text, known)) if same_font(known, &fonts[font]) => {
                            text.push(character);
                        }
                        _ => word.push((character.to_string(), fonts[font].clone())),
                    }
                }
            }
            Piece::Tab(font) => {
                flush(&mut word, &mut tokens);
                tokens.push(Token::Tab(fonts[font].clone()));
            }
            Piece::Picture { width, height } => {
                flush(&mut word, &mut tokens);
                tokens.push(Token::Picture { width, height });
            }
        }
    }
    flush(&mut word, &mut tokens);
    Some(tokens)
}

/// Whether two fonts measure the same, so their pieces of one word can be one piece.
#[expect(
    clippy::float_cmp,
    reason = "two runs are measured with the same font exactly when their sizes are the same value"
)]
fn same_font(one: &Font, other: &Font) -> bool {
    one.name == other.name
        && one.size == other.size
        && one.bold == other.bold
        && one.italic == other.italic
        && one.character_spacing == other.character_spacing
        && one.scale == other.scale
        && one.monospaced == other.monospaced
}

/// A field's number as text: what the pass wrote into it, or nothing when it has none.
fn field_text(field: &Field, pass: &Pass<'_>) -> String {
    match field {
        Field::PageReference(name) => pass
            .numbers
            .get(name)
            .map(ToString::to_string)
            .unwrap_or_default(),
        Field::PageCount => pass
            .page_count
            .map(|count| count.to_string())
            .unwrap_or_default(),
        Field::PageNumber => String::new(),
    }
}

/// One column of a table, as the reference sizes it: the widest token its cells must hold on a line,
/// and the whole of their text on one line, both with the cells' margins.
#[derive(Clone, Copy)]
struct Column {
    min: f64,
    width: f64,
}

/// A table's columns as the reference sizes them, in points, from the content of their cells.
fn columns_of(table: &Table, pass: &Pass<'_>) -> Option<Vec<Column>> {
    let count = table
        .rows
        .iter()
        .map(|row| row.cells.len())
        .max()
        .unwrap_or(0);
    let margin = table.margin_left + table.margin_right;
    let mut columns = Vec::with_capacity(count);
    for index in 0..count {
        let mut min = 0.0_f64;
        let mut max = 0.0_f64;
        for cell in table.rows.iter().filter_map(|row| row.cells.get(index)) {
            let (own_min, own_max) = content_widths(cell, pass)?;
            min = min.max(own_min + margin);
            max = max.max(own_max + margin);
        }
        columns.push(Column {
            min,
            width: max.max(min),
        });
    }
    Some(columns)
}

/// How narrow and how wide a cell's content can be, in points, without its margins: the widest token
/// a line must hold, and the whole of its text on one line.
fn content_widths(cell: &Cell, pass: &Pass<'_>) -> Option<(f64, f64)> {
    let mut min = 0.0_f64;
    let mut max = 0.0_f64;
    for block in &cell.blocks {
        let (own_min, own_max) = match block {
            Block::Paragraph(paragraph) => paragraph_widths(paragraph, pass)?,
            // A table inside a cell is as wide as its own columns, as the reference measures it.
            Block::Table(inner) => {
                let columns = columns_of(inner, pass)?;
                let borders = f64::midpoint(inner.border_left, inner.border_right);
                (
                    columns.iter().map(|column| column.min).sum::<f64>() + borders,
                    columns.iter().map(|column| column.width).sum::<f64>() + borders,
                )
            }
        };
        min = min.max(own_min);
        max = max.max(own_max);
    }
    Some((min, max))
}

/// How narrow and how wide a paragraph's text can be, in points: the widest token it must hold on a
/// line, and the whole of it on one line, with its indents.
fn paragraph_widths(paragraph: &Paragraph, pass: &Pass<'_>) -> Option<(f64, f64)> {
    let tokens = tokens_of(paragraph, pass)?;
    let format = &paragraph.format;
    let left = format.indent_left.max(0.0);
    let right = format.indent_right.max(0.0);
    let mut position = left + format.first_line;
    let mut min = 0.0_f64;
    let mut first = true;
    for token in tokens {
        // The first token starts where the first line starts; every later one where a line starts.
        let start = if first { position } else { left };
        match token {
            Token::Word(pieces) => {
                let width = width_of(&pieces, position);
                min = min.max(start + width + right);
                position += width;
            }
            Token::Space(font) => position += measure_text_width(" ", &font, position),
            Token::Tab(_) => position = next_tab(format, position),
            Token::Picture { width, .. } => {
                min = min.max(start + width + right);
                position += width;
            }
        }
        first = false;
    }
    Some((min, (position + right).max(min)))
}

/// The width each column of a table takes, as the reference fits it: each column as wide as its
/// content, then widened or narrowed to the width the table gives itself, less half its borders.
fn column_widths(table: &Table, available: f64, pass: &Pass<'_>) -> Option<Vec<f64>> {
    let columns = columns_of(table, pass)?;
    sized(&columns, target_of(table, available), available, table)
}

/// The width a table gives itself, before its borders are taken off.
fn target_of(table: &Table, available: f64) -> f64 {
    match table.width {
        Some(share) => share * available,
        None => available,
    }
}

/// The reference's fitting of a table's columns to the width it is given, or `None` at the stop: a
/// table whose cells' margins alone are wider than the room for it.
fn sized(columns: &[Column], target: f64, available: f64, table: &Table) -> Option<Vec<f64>> {
    let borders = f64::midpoint(table.border_left, table.border_right);
    let room = target - borders;
    let total: f64 = columns.iter().map(|column| column.width).sum();
    let least: f64 = columns.iter().map(|column| column.min).sum();
    let margin = table.margin_left + table.margin_right;
    #[expect(
        clippy::cast_precision_loss,
        reason = "a table's column count is a small whole number, exactly representable"
    )]
    let margins = margin * columns.len() as f64;
    if least > room {
        // The widest tokens alone do not fit: a table that fills a share of the width it is in
        // shares what is left of it among them, and one whose cells' margins alone are wider than
        // the room is what the reference stops at.
        let shared_room = match table.width {
            Some(share) if share < 1.0 => available - borders,
            _ => room,
        };
        if least <= shared_room {
            return Some(columns.iter().map(|column| column.min).collect());
        }
        if margins > shared_room {
            return None;
        }
        let words: f64 = columns
            .iter()
            .map(|column| (column.min - margin).max(0.0))
            .sum();
        return Some(
            columns
                .iter()
                .map(|column| {
                    margin + (shared_room - margins) * (column.min - margin).max(0.0) / words
                })
                .collect(),
        );
    }
    // The reference widens a table whose columns are narrower than the room it gives itself, and only
    // when there is a width to share out — its own test is `total > spacing`, the room between cells,
    // which a created table states none of. Nothing a created table can state makes `total` zero: a
    // column carries its cells' margins, so even a column of empty cells is wider than nothing. A
    // table with no columns, or one whose cells state no margins, would keep its columns as they are.
    if total > 0.0 && total < room {
        return Some(
            columns
                .iter()
                .map(|column| column.width * room / total)
                .collect(),
        );
    }
    // The columns already fill the room, or there is no width to share out at all: they stay as they
    // are.
    if total <= room {
        return Some(columns.iter().map(|column| column.width).collect());
    }
    if least >= room {
        return Some(columns.iter().map(|column| column.min).collect());
    }
    Some(
        columns
            .iter()
            .map(|column| {
                column.min + (column.width - column.min) * (room - least) / (total - least)
            })
            .collect(),
    )
}

/// A table's rows, each with its cells' lines and the room its margins and borders take.
fn table_rows(table: &Table, available: f64, pass: &Pass<'_>) -> Option<Vec<RowLines>> {
    let widths = column_widths(table, available, pass)?;
    let last = table.rows.len().saturating_sub(1);
    let mut rows = Vec::new();
    for (index, row) in table.rows.iter().enumerate() {
        let mut cells = Vec::new();
        for (cell, width) in row.cells.iter().zip(&widths) {
            let room = width - table.margin_left - table.margin_right;
            let mut lines = Vec::new();
            for block in &cell.blocks {
                match block {
                    Block::Paragraph(paragraph) => {
                        lines.extend(paragraph_lines(paragraph, room, pass)?);
                    }
                    // A table inside a cell is placed as one block of its own height. A created cell
                    // holds text only, so nothing here can reach it.
                    Block::Table(inner) => {
                        let mut height = 0.0;
                        for inner_row in table_rows(inner, room, pass)? {
                            height +=
                                inner_row.height_of(inner_row.cells.iter().map(Vec::as_slice));
                        }
                        lines.push(Line {
                            height,
                            page_break_before: false,
                        });
                    }
                }
            }
            cells.push(lines);
        }
        rows.push(RowLines {
            cells,
            margins: table.margin_top + table.margin_bottom,
            // The border above the row — the table's top border over its first row, its inside border
            // between the rest — and, below the last row, the table's bottom border. The reference
            // counts no bottom border on any other row; the border a break draws below it is
            // `break_border`.
            borders: (if index == 0 {
                table.border_top
            } else {
                table.border_inside
            }) + if index == last {
                table.border_bottom
            } else {
                0.0
            },
            break_border: if index < last {
                table.border_bottom
            } else {
                0.0
            },
        });
    }
    Some(rows)
}

/// The passes: lay the document out with the numbers of the pass before until they stop changing.
pub(super) fn passes(content: &Content) -> Option<super::Estimate> {
    passes_until(content, pass_cap())
}

/// The passes, at most `cap` of them: the seam a test uses to force the numbers that never settle,
/// which no recorded case reaches.
fn passes_until(content: &Content, cap: u32) -> Option<super::Estimate> {
    let mut numbers: BTreeMap<String, u32> = BTreeMap::new();
    let mut page_count: Option<u32> = None;
    let mut last: Option<super::Estimate> = None;
    for _ in 0..cap {
        let pass = Pass {
            numbers: &numbers,
            page_count,
            ideographs: content.ideographs(),
        };
        let estimate = lay_out(content, &pass);
        // Two passes that agree worked the same numbers out, which is when the layout has settled.
        if last.as_ref() == Some(&estimate) {
            return Some(estimate);
        }
        numbers.clone_from(&estimate.bookmark_numbers);
        page_count = estimate.page_count;
        last = Some(estimate);
    }
    // The numbers were still changing after the last pass the reference allows, so none of them can
    // be written.
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::writing::docx::layout::read::{Item, Page, Paragraph, Run, points};

    /// A document of `paragraphs` one-line paragraphs on the page `create` writes, in the mode the
    /// created settings state.
    fn content(paragraphs: usize) -> Content {
        let mut blocks = Vec::new();
        for index in 0..paragraphs {
            let mut paragraph = Paragraph::default();
            paragraph.items.push(Item::Text(Run {
                text: format!("Line {index}"),
                font: Font::default(),
            }));
            paragraph.bookmark = Some(format!("_Toc{}", index + 1));
            blocks.push(Block::Paragraph(paragraph));
        }
        let page = crate::writing::docx::blocks::CREATED_PAGE;
        Content {
            page: Page {
                width: points(page.width),
                height: points(page.height),
                top: points(page.top),
                right: points(page.right),
                bottom: points(page.bottom),
                left: points(page.left),
                header: points(crate::writing::docx::blocks::CREATED_HEADER_TWIPS),
                footer: points(crate::writing::docx::blocks::CREATED_HEADER_TWIPS),
            },
            blocks,
            header: Vec::new(),
            footer: Vec::new(),
            footnotes: Vec::new(),
            compatibility_mode: super::super::read::compatibility_mode(
                include_str!("../../../../assets/docx-create/settings.xml").as_bytes(),
            )
            .expect("the created settings state a mode"),
        }
    }

    /// The width the created page leaves a paragraph, in points.
    fn column() -> f64 {
        let page = crate::writing::docx::blocks::CREATED_PAGE;
        points(page.width) - points(page.left) - points(page.right)
    }

    /// A table with the cells a test gives, at the margins, borders and width the writer's own table
    /// states. Those numbers are read from the fixture document rather than restated here, so a test
    /// that changes the writer's table changes what every measurement below is of.
    fn table(rows: &[Vec<&str>]) -> Table {
        let parts = crate::test_support::docx_fixture::parts(0, 1);
        let written = Content::read(&parts)
            .expect("the fixture's parts are readable")
            .blocks
            .into_iter()
            .find_map(|block| match block {
                Block::Table(table) => Some(table),
                Block::Paragraph(_) => None,
            })
            .expect("the fixture document holds a table");
        Table {
            rows: rows
                .iter()
                .map(|row| crate::writing::docx::layout::read::Row {
                    cells: row
                        .iter()
                        .map(|text| crate::writing::docx::layout::read::Cell {
                            blocks: vec![Block::Paragraph(Paragraph {
                                items: vec![Item::Text(Run {
                                    text: (*text).to_string(),
                                    font: Font::default(),
                                })],
                                ..Paragraph::default()
                            })],
                        })
                        .collect(),
                })
                .collect(),
            ..written
        }
    }

    #[test]
    fn a_table_s_columns_are_as_wide_as_their_content_as_the_reference_fits_them() {
        // The widths the reference's own fitting gives this table: each column as wide as its widest
        // line, then widened to the text column less half the table's borders, which is the rule
        // `probes/table.mjs` shows the reference applying — a row of these unequal cells is one line
        // tall, where a table of the grid's equal columns makes the same text two. A table whose
        // columns were its grid's would give each of these two the same width.
        let table = table(&[
            vec!["alpha beta gamma delta epsilon zeta eta theta", "one"],
            vec!["two", "three"],
        ]);
        let numbers = BTreeMap::new();
        let pass = Pass {
            numbers: &numbers,
            page_count: None,
            ideographs: true,
        };
        let widths = column_widths(&table, column(), &pass).expect("the columns");
        assert!((widths[0] - 388.38).abs() < 0.01, "{widths:?}");
        assert!((widths[1] - 62.42).abs() < 0.01, "{widths:?}");
        // The two columns and the half border the table takes on each side are the text column.
        let borders = f64::midpoint(table.border_left, table.border_right);
        assert!(
            (widths.iter().sum::<f64>() - (column() - borders)).abs() < 0.01,
            "{widths:?}"
        );
    }

    #[test]
    fn a_table_whose_margins_exceed_the_room_for_it_stops_the_layout() {
        // The reference stops at forty-two columns and lays forty-one out (`probes/table.mjs` reports
        // both): the cells' margins alone are wider than the room a column gets.
        let wide = |columns: usize| table(&[vec!["x"; columns]]);
        let one = wide(1);
        let margins = one.margin_left + one.margin_right;
        let numbers = BTreeMap::new();
        let pass = Pass {
            numbers: &numbers,
            page_count: None,
            ideographs: true,
        };
        let laid =
            column_widths(&wide(41), column(), &pass).expect("forty-one columns are laid out");
        assert_eq!(laid.len(), 41);
        assert!(laid[0] > margins, "{laid:?}");
        assert!(column_widths(&wide(42), column(), &pass).is_none());
    }

    #[test]
    fn a_row_counts_the_border_above_it_and_the_one_a_break_draws_below_it() {
        // The reference counts the border above every row it places, and the table's bottom border
        // below a row that breaks; the last row carries that bottom border instead of a break. A port
        // that counted no border would fit more rows on a page than the reference does, so the table
        // below is one row longer than the reference's page holds and the extra row must take a
        // second page.
        let numbers = BTreeMap::new();
        let pass = Pass {
            numbers: &numbers,
            page_count: None,
            ideographs: true,
        };
        let grid = table(&[vec!["x"], vec!["x"]]);
        let rows = table_rows(&grid, column(), &pass).expect("the rows are laid out");
        let line = measure_line_height(&Font::default());
        assert!(
            (rows[0].borders - grid.border_top).abs() < 1e-9,
            "{}",
            rows[0].borders
        );
        assert!(
            (rows[1].borders - (grid.border_inside + grid.border_bottom)).abs() < 1e-9,
            "{}",
            rows[1].borders
        );
        assert!(
            (rows[0].height_of(rows[0].cells.iter().map(Vec::as_slice)) - (line + grid.border_top))
                .abs()
                < 1e-9
        );
        assert!((rows[0].break_border - grid.border_bottom).abs() < 1e-9);
        assert!(rows[1].break_border.abs() < 1e-9);
        let page = crate::writing::docx::blocks::CREATED_PAGE;
        let body = points(page.height) - points(page.top) - points(page.bottom);
        // A page holds the first row, whose border is the table's top one, and then as many rows as
        // fit below it, each with the inside border — less the border a break draws under the last.
        #[expect(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "a page holds a small whole number of rows"
        )]
        let fits = ((body - grid.border_bottom - (line + grid.border_top))
            / (line + grid.border_inside))
            .floor() as usize
            + 1;
        let document = |count: usize| {
            let mut document = content(0);
            document
                .blocks
                .push(Block::Table(table(&vec![vec!["x"]; count])));
            document
        };
        assert_eq!(
            passes(&document(fits))
                .expect("a table of one page")
                .page_count,
            Some(1)
        );
        assert_eq!(
            passes(&document(fits + 1))
                .expect("a table one row too long")
                .page_count,
            Some(2)
        );
    }

    #[test]
    fn a_row_taller_than_the_room_left_on_a_page_breaks_across_it() {
        // Ten paragraphs, a table whose one cell holds a great deal of text, and twenty paragraphs
        // after it. The row's lines break across the page they reach, as the reference lays them out:
        // the part that fits stays on the first page, the rest goes on from the top of the second,
        // and the twenty paragraphs follow it there — two pages. A port that moved the whole row to
        // the next page would leave the first page holding only the ten paragraphs, put the row on the
        // second and the twenty paragraphs on a third.
        let long = crate::test_support::docx_fixture::tall_cell_text();
        let mut document = content(10);
        document
            .blocks
            .push(Block::Table(table(&[vec![long.as_str(), "1"]])));
        let after = content(20);
        document.blocks.extend(after.blocks);
        let estimate = passes(&document).expect("the layout works out the document");
        assert_eq!(estimate.page_count, Some(2));
    }

    #[test]
    fn a_short_document_is_one_page() {
        let estimate = passes(&content(3)).expect("the layout works out a short document");
        assert_eq!(estimate.page_count, Some(1));
        assert_eq!(estimate.bookmark_numbers.get("_Toc1"), Some(&1));
    }

    #[test]
    fn a_long_document_breaks_onto_another_page() {
        // The body is 697.9 points and a line 11.499, so sixty fit; 100 paragraphs take two pages.
        let estimate = passes(&content(100)).expect("the layout works out a long document");
        assert_eq!(estimate.page_count, Some(2));
        assert_eq!(estimate.bookmark_numbers.get("_Toc1"), Some(&1));
        assert_eq!(estimate.bookmark_numbers.get("_Toc61"), Some(&2));
    }

    /// A document whose page reference moves the bookmark it names: the first paragraph's word
    /// fills the column, so the number the reference writes wraps it onto a second line.
    fn content_with_a_page_reference() -> Content {
        let mut document = content(60);
        let font = Font::default();
        let mut text = String::new();
        while measure_text_width(&text, &font, 0.0) <= column() {
            text.push('a');
        }
        text.pop();
        let mut paragraph = Paragraph::default();
        paragraph.items.push(Item::Text(Run { text, font }));
        paragraph
            .items
            .push(Item::Field(Field::PageReference("_Toc60".to_string())));
        document.blocks[0] = Block::Paragraph(paragraph);
        document
    }

    #[test]
    fn numbers_still_moving_after_the_last_pass_are_answered_with_none() {
        // The numbers genuinely move: the first pass writes nothing into the page reference, because
        // it has no numbers yet, so the paragraph that carries it is one line and `_Toc60` stays on
        // the first page. The second pass writes that page's number, which wraps the paragraph onto a
        // second line and pushes `_Toc60` onto the second page; the third pass sees the same layout
        // and settles. Two passes are not enough, three are.
        let document = content_with_a_page_reference();
        assert!(passes_until(&document, 2).is_none());
        let estimate = passes(&document).expect("the layout settles on its third pass");
        assert_eq!(estimate.bookmark_numbers.get("_Toc60"), Some(&2));
        assert_eq!(estimate.page_count, Some(2));
    }
}

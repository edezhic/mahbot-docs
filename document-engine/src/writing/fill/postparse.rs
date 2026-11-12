//! Pairing sections, expanding them to the element they wrap, expanding a raw tag to the paragraph
//! it replaces, and keeping the significant space a Word text element needs.

use std::collections::{HashMap, HashSet};

use super::content::Config;
use super::errors::{self, TemplateError};
use super::parts::{
    self, Expand, Kind, Part, Placeholder, Position, is_paragraph_end, is_paragraph_start,
    is_starting, is_tag_end, is_tag_start, is_text_end, is_text_start, tag_name,
};
use crate::vocabulary::Family;

/// What one part's postparse needs beside its own pieces.
pub(super) struct Context<'a> {
    pub config: &'static Config,
    pub tags: HashSet<String>,
    pub bound: usize,
    /// The section properties every section walk reads, taken once from the last Word main-type
    /// templated part as the reference's preparse pass leaves them.
    pub sects: &'a [Vec<Part>],
}

impl<'a> Context<'a> {
    pub fn new(config: &'static Config, bound: usize, sects: &'a [Vec<Part>]) -> Self {
        Self {
            config,
            tags: HashSet::new(),
            bound,
            sects,
        }
    }
}

/// A postparse failure: the library's own template errors, or the engine's own nesting bound.
pub(super) enum Error {
    Template(Vec<TemplateError>),
    Nested(usize),
}

impl From<Vec<TemplateError>> for Error {
    fn from(errors: Vec<TemplateError>) -> Self {
        Self::Template(errors)
    }
}

/// Postparse one level of a part, recursing into every section's body.
pub(super) fn postparse(
    parts: Vec<Part>,
    ctx: &mut Context,
    depth: usize,
    base: Option<&mut Placeholder>,
) -> Result<Vec<Part>, Error> {
    if depth > ctx.bound {
        return Err(Error::Nested(depth));
    }
    let mut base = base;
    let parts = loop_postparse(parts, ctx, base.as_deref_mut());
    let parts = space_preserve(parts, base.as_deref(), ctx.config);
    // The reference's modules run one after the other and each answers the parts it leaves, so a
    // pairing failure does not hide the raw tags the next module refuses: every module's errors are
    // collected and joined.
    let mut errors = Vec::new();
    let parts = match expand_pairs(&parts, ctx, depth) {
        Ok(expanded) => expanded,
        Err(Error::Nested(depth)) => return Err(Error::Nested(depth)),
        Err(Error::Template(found)) => {
            errors.extend(found);
            parts
        }
    };
    let (parts, raw_errors) = raw_xml(parts, ctx);
    errors.extend(raw_errors);
    for part in &parts {
        if let Part::Placeholder(placeholder) = part {
            ctx.tags.insert(placeholder.value.clone());
        }
    }
    if errors.is_empty() {
        Ok(parts)
    } else {
        Err(errors.into())
    }
}

// ── the loop module's postparse ─────────────────────────────────────────────────────────────────

/// The Word section bookkeeping and the paragraph-loop trimming the reference's loop module does.
fn loop_postparse(
    parts: Vec<Part>,
    ctx: &mut Context,
    base: Option<&mut Placeholder>,
) -> Vec<Part> {
    let Some(base) = base else {
        return parts;
    };
    if ctx.config.family == Family::Docx && !parts.is_empty() {
        // The reference walks the main part's section properties for two things: a next-page
        // section that starts inside the pair, which one rendered copy still inserts, and the
        // bookkeeping a repeated copy uses to keep or drop its header and footer references.
        // No value this operation accepts repeats a section, so only the first is reachable.
        for (index, sect) in ctx.sects.iter().enumerate() {
            let Some(first) = sect.first() else {
                continue;
            };
            if parts[0].l_index() < first.l_index() && first.l_index() < base.l_index {
                if is_next_page(sect) {
                    base.next_page = Some(index);
                }
                break;
            }
        }
        base.last_paragraph_sect_pr = last_sect_pr(&parts);
    }
    if !matches!(
        base.kind,
        Kind::Open {
            expand: Expand::Auto,
            ..
        }
    ) || !is_enclosed_by_paragraphs(&parts)
    {
        return parts;
    }
    base.paragraph_loop = true;
    let mut level = 0isize;
    let chunks = chunk_by(&parts, |part| paragraph_boundary(part, &mut level));
    let first_chunk = chunks.first().cloned().unwrap_or_default();
    let last_chunk = chunks.last().cloned().unwrap_or_default();
    let mut first_offset = offset(&first_chunk);
    let mut last_offset = offset(&last_chunk);
    if first_offset > 0
        && chunks
            .get(1)
            .and_then(|chunk| chunk.first())
            .is_some_and(is_whitespace)
    {
        first_offset += 1;
    }
    if last_offset > 0
        && chunks
            .len()
            .checked_sub(2)
            .and_then(|index| chunks.get(index))
            .and_then(|chunk| chunk.last())
            .is_some_and(is_whitespace)
    {
        last_offset += 1;
    }
    base.page_break_beginning = has_page_break(&first_chunk);
    base.page_break = has_page_break(&last_chunk);
    if has_image(&first_chunk) {
        first_offset = 0;
    }
    if has_image(&last_chunk) {
        last_offset = 0;
    }
    let end = parts.len().saturating_sub(last_offset);
    if first_offset >= end {
        Vec::new()
    } else {
        parts[first_offset..end].to_vec()
    }
}

/// Whether a chunk holds text a template tag may sit in.
fn has_content(chunk: &[Part]) -> bool {
    chunk.iter().any(Part::is_content)
}

/// The offset a paragraph chunk contributes to a paragraph loop.
fn offset(chunk: &[Part]) -> usize {
    if has_content(chunk) { 0 } else { chunk.len() }
}

/// [`parts::is_whitespace`] for a part that may be markup.
fn is_whitespace(part: &Part) -> bool {
    matches!(part, Part::Text { value, .. } if parts::is_whitespace(value))
}

/// Whether `chunk` holds a page break.
fn has_page_break(chunk: &[Part]) -> bool {
    chunk.iter().any(|part| {
        part.tag()
            .is_some_and(|tag| tag.name == "w:br" && tag.value.contains("w:type=\"page\""))
    })
}

/// Whether `chunk` holds a drawing.
fn has_image(chunk: &[Part]) -> bool {
    chunk.iter().any(|part| is_tag_start(part, "w:drawing"))
}

/// Whether the parts begin and end with whole paragraphs.
fn is_enclosed_by_paragraphs(parts: &[Part]) -> bool {
    parts.first().is_some_and(is_paragraph_start) && parts.last().is_some_and(is_paragraph_end)
}

/// Where a chunk boundary falls in the reference's `chunkBy`.
enum Boundary {
    /// A chunk opens before this part.
    Start,
    /// A chunk closes after this part.
    End,
    /// The part stays in the chunk being built.
    Inside,
}

/// The parts grouped by a marker function, as the reference's `chunkBy` groups them.
fn chunk_by<F>(parts: &[Part], mut marker: F) -> Vec<Vec<Part>>
where
    F: FnMut(&Part) -> Boundary,
{
    let mut chunks: Vec<Vec<Part>> = vec![Vec::new()];
    for part in parts {
        let current = chunks.last_mut().expect("one chunk is always present");
        match marker(part) {
            Boundary::Start => chunks.push(vec![part.clone()]),
            Boundary::End => {
                current.push(part.clone());
                chunks.push(Vec::new());
            }
            Boundary::Inside => current.push(part.clone()),
        }
    }
    // The reference drops the empty chunk before a leading start and the one after a trailing end,
    // which is what makes a body that begins and ends with a paragraph two chunks, not four.
    chunks.retain(|chunk| !chunk.is_empty());
    chunks
}

/// The reference's paragraph-loop chunk marker: only a level-1 paragraph start opens a chunk and
/// only a level-0 paragraph end closes one, so a paragraph inside a text box stays in its parent's
/// chunk.
fn paragraph_boundary(part: &Part, level: &mut isize) -> Boundary {
    if is_paragraph_start(part) {
        *level += 1;
        if *level == 1 {
            return Boundary::Start;
        }
    }
    if is_paragraph_end(part) {
        *level -= 1;
        if *level == 0 {
            return Boundary::End;
        }
    }
    Boundary::Inside
}

/// The section properties the last paragraph of `parts` states.
fn last_sect_pr(parts: &[Part]) -> Option<String> {
    let mut sect: Vec<String> = Vec::new();
    let mut inside = false;
    for part in parts.iter().rev() {
        if is_tag_end(part, "w:sectPr") {
            inside = true;
        }
        if is_tag_start(part, "w:sectPr") {
            sect.insert(0, part.text().to_string());
            inside = false;
        }
        if inside {
            sect.insert(0, part.text().to_string());
        }
        if is_paragraph_start(part) {
            if !sect.is_empty() {
                return Some(sect.concat());
            }
            break;
        }
    }
    None
}

/// The section properties a part states, in document order.
pub(super) fn get_sect_pr(parts: &[Part]) -> Vec<Vec<Part>> {
    let mut sects: Vec<Vec<Part>> = Vec::new();
    let mut open = false;
    for part in parts {
        if is_tag_start(part, "w:sectPr") {
            sects.push(Vec::new());
            open = true;
        }
        if open && let Some(sect) = sects.last_mut() {
            sect.push(part.clone());
        }
        if is_tag_end(part, "w:sectPr") {
            open = false;
        }
    }
    sects
}

fn is_next_page(parts: &[Part]) -> bool {
    parts
        .iter()
        .any(|part| is_tag_start(part, "w:type") && part.text().contains("w:val=\"nextPage\""))
}

// ── the space-preserve module's postparse ───────────────────────────────────────────────────────

/// Keep `xml:space="preserve"` on a Word text element whose content a render may trim.
fn space_preserve(parts: Vec<Part>, base: Option<&Placeholder>, config: &Config) -> Vec<Part> {
    if !config.space_preserve {
        return parts;
    }
    let mut result = Vec::new();
    let mut chunk: Vec<Part> = Vec::new();
    let mut in_text = false;
    let mut end_l_index = 0;
    let mut last_text_tag = 0;
    for part in parts {
        if is_text_start(&part) && part.tag().is_some_and(|tag| tag.name == "w:t") {
            in_text = true;
            last_text_tag = chunk.len();
        }
        if !in_text {
            result.push(part);
            continue;
        }
        chunk.push(part.clone());
        if base.is_some() && chunk.len() > 1 {
            end_l_index = base.map_or(0, |base| base.end_l_index);
            add_preserve(&mut chunk, 0);
        }
        if let Part::Placeholder(placeholder) = &part
            && chunk.len() > 1
        {
            add_preserve(&mut chunk, last_text_tag);
            end_l_index = placeholder.end_l_index;
        }
        if is_text_end(&part) && part.l_index() > end_l_index {
            if end_l_index != 0 {
                add_preserve(&mut chunk, last_text_tag);
            }
            result.append(&mut chunk);
            in_text = false;
            end_l_index = 0;
            last_text_tag = 0;
        }
    }
    result.append(&mut chunk);
    result
}

/// Add `xml:space="preserve"` to a text element's open tag when it has content.
fn add_preserve(chunk: &mut [Part], index: usize) {
    let Some(next) = chunk.get(index + 1) else {
        return;
    };
    if next.text() == "</w:t>" {
        return;
    }
    let Some(Part::Tag(tag)) = chunk.get_mut(index) else {
        return;
    };
    if tag.value.contains("xml:space=\"preserve\"") {
        return;
    }
    if let Some(position) = tag.value.rfind('>') {
        tag.value.insert_str(position, " xml:space=\"preserve\"");
    }
}

// ── the pair-expansion module's postparse ───────────────────────────────────────────────────────

/// Pair every section's opening and closing tag and replace the pair with the section it wraps.
fn expand_pairs(parts: &[Part], ctx: &mut Context, depth: usize) -> Result<Vec<Part>, Error> {
    let mut traits: Vec<usize> = parts
        .iter()
        .enumerate()
        .filter(|(_, part)| {
            part.placeholder().is_some_and(|placeholder| {
                placeholder.sub.is_none()
                    && matches!(placeholder.kind, Kind::Open { .. } | Kind::Close)
            })
        })
        .map(|(index, _)| index)
        .collect();
    traits.sort_by_key(|index| parts[*index].placeholder().map_or(0, |part| part.offset));
    let pairs = get_pairs(&traits, parts)?;
    let expanded = pair_edges(&pairs, parts, ctx.config)?;
    let mut errors: Vec<TemplateError> = Vec::new();
    let mut result = Vec::with_capacity(parts.len());
    let mut cursor = 0;
    for ((open_index, close_index), (left, right)) in pairs.iter().zip(&expanded) {
        if *left < cursor {
            continue;
        }
        result.extend(parts[cursor..*left].iter().cloned());
        let mut inner = Vec::new();
        for (index, part) in parts.iter().enumerate().take(right + 1).skip(*left) {
            if index == *open_index || index == *close_index {
                continue;
            }
            inner.push(part.clone());
        }
        let mut base = parts[*open_index]
            .placeholder()
            .cloned()
            .expect("a trait is a placeholder");
        base.end_l_index = parts[*close_index].l_index();
        match postparse(inner, ctx, depth + 1, Some(&mut base)) {
            Ok(sub) => base.sub = Some(sub),
            Err(Error::Nested(depth)) => return Err(Error::Nested(depth)),
            Err(Error::Template(found)) => errors.extend(found),
        }
        result.push(Part::Placeholder(base));
        cursor = right + 1;
    }
    result.extend(parts[cursor..].iter().cloned());
    if errors.is_empty() {
        Ok(result)
    } else {
        Err(errors.into())
    }
}

/// Each pair's edges, or every failure the reference collects looking one up: an expand target that
/// would split the sample's own XML, a missing left or right edge, and a pair that starts inside the
/// previous one.
fn pair_edges(
    pairs: &[(usize, usize)],
    parts: &[Part],
    config: &Config,
) -> Result<Vec<(usize, usize)>, Vec<TemplateError>> {
    let mut errors: Vec<TemplateError> = Vec::new();
    let mut expanded: Vec<Option<(usize, usize)>> = Vec::with_capacity(pairs.len());
    let mut last_right = 0;
    let mut last_pair: Option<(usize, usize)> = None;
    for (open_index, close_index) in pairs {
        let open = parts[*open_index]
            .placeholder()
            .expect("a trait is a placeholder");
        let expand = match &open.kind {
            Kind::Open { expand, .. } => match expand {
                Expand::Named(tag) => Some(tag.clone()),
                Expand::Auto => {
                    match get_expand_to_default(parts, *open_index, *close_index, config) {
                        Ok(tag) => tag,
                        Err(found) => {
                            errors.extend(found);
                            None
                        }
                    }
                }
            },
            _ => None,
        };
        let edges = match expand.as_deref() {
            Some(tag) => {
                let left = get_left_or_null(parts, tag, *open_index);
                if left.is_none() {
                    errors.push(errors::tag_not_found(tag, true));
                }
                let right = get_right_or_null(parts, tag, *close_index);
                if right.is_none() {
                    errors.push(errors::tag_not_found(tag, false));
                }
                left.zip(right)
            }
            None => Some((*open_index, *close_index)),
        };
        if let Some((left, right)) = edges {
            if left < last_right {
                let (last_open, last_close) = last_pair.unwrap_or((*open_index, *close_index));
                errors.push(errors::unbalanced_loops(
                    &parts[last_open]
                        .placeholder()
                        .map_or(String::new(), |part| part.value.clone()),
                    &parts[last_close]
                        .placeholder()
                        .map_or(String::new(), |part| part.value.clone()),
                    &open.value,
                    &parts[*close_index]
                        .placeholder()
                        .map_or(String::new(), |part| part.value.clone()),
                ));
            }
            last_right = right;
        }
        last_pair = Some((*open_index, *close_index));
        expanded.push(edges);
    }
    if !errors.is_empty() {
        return Err(errors);
    }
    Ok(expanded
        .into_iter()
        .map(|edges| edges.expect("a pair without an error has both edges"))
        .collect())
}

/// The section pairs the reference's own transformer finds, or the first errors it reports.
fn get_pairs(traits: &[usize], parts: &[Part]) -> Result<Vec<(usize, usize)>, Vec<TemplateError>> {
    let mut remaining: Vec<usize> = traits.to_vec();
    let mut errors = Vec::new();
    while !remaining.is_empty() {
        match transformer(&mut remaining, parts) {
            Ok(()) => {}
            Err(mut found) => errors.append(&mut found),
        }
    }
    if !errors.is_empty() {
        return Err(errors);
    }
    let mut level: HashMap<usize, usize> = HashMap::new();
    let mut count_open: isize = 0;
    let mut pairs = Vec::new();
    for index in traits {
        let open = parts[*index]
            .placeholder()
            .is_some_and(|placeholder| matches!(placeholder.kind, Kind::Open { .. }));
        count_open += if open { 1 } else { -1 };
        if open {
            level.insert(usize::try_from(count_open).unwrap_or(0), *index);
        } else {
            let start = usize::try_from(count_open + 1)
                .ok()
                .and_then(|level_index| level.get(&level_index))
                .copied();
            if count_open == 0
                && let Some(start) = start
            {
                pairs.push((start, *index));
            }
        }
        count_open = count_open.max(0);
    }
    Ok(pairs)
}

/// Remove one pair from `traits`, or answer the first error the reference reports.
fn transformer(traits: &mut Vec<usize>, parts: &[Part]) -> Result<(), Vec<TemplateError>> {
    let mut index = 0;
    while index < traits.len() {
        let part = parts[traits[index]]
            .placeholder()
            .expect("a trait is a placeholder");
        if matches!(part.kind, Kind::Close) {
            if index == 0 {
                let removed = traits.remove(0);
                return Err(vec![errors::unmatched_loop(
                    parts[removed]
                        .placeholder()
                        .expect("a trait is a placeholder"),
                )]);
            }
            let end_index = index;
            let start_index = index - 1;
            if pair_matches(traits[start_index], traits[end_index], parts) {
                traits.remove(end_index);
                traits.remove(start_index);
                return Ok(());
            }
            let mut offseter = 1;
            while offseter < 50 {
                if start_index >= offseter
                    && pair_matches(traits[start_index - offseter], traits[end_index], parts)
                {
                    traits.remove(end_index);
                    traits.remove(start_index - offseter);
                    return Ok(());
                }
                if end_index + offseter < traits.len()
                    && pair_matches(traits[start_index], traits[end_index + offseter], parts)
                {
                    traits.remove(end_index + offseter);
                    traits.remove(start_index);
                    return Ok(());
                }
                offseter += 1;
            }
            let open = parts[traits[start_index]]
                .placeholder()
                .map_or("", |part| part.value.as_str());
            let close = parts[traits[end_index]]
                .placeholder()
                .map_or("", |part| part.value.as_str());
            traits.remove(end_index);
            traits.remove(start_index);
            return Err(vec![errors::closing_mismatch(open, close)]);
        }
        index += 1;
    }
    let mut errors = Vec::new();
    for index in traits.iter() {
        let part = parts[*index]
            .placeholder()
            .expect("a trait is a placeholder");
        errors.push(errors::unmatched_loop(part));
    }
    traits.clear();
    Err(errors)
}

/// Whether one opening trait closes with one closing trait.
fn pair_matches(start: usize, end: usize, parts: &[Part]) -> bool {
    let Some(start) = parts[start].placeholder() else {
        return false;
    };
    let Some(end) = parts[end].placeholder() else {
        return false;
    };
    matches!(start.kind, Kind::Open { .. })
        && matches!(end.kind, Kind::Close)
        && (start.value == end.value || end.value.is_empty())
}

/// The element an automatic section expands to, or the reference's error when the tags are placed
/// so no element wraps them.
fn get_expand_to_default(
    parts: &[Part],
    open_index: usize,
    close_index: usize,
    config: &Config,
) -> Result<Option<String>, Vec<TemplateError>> {
    let open = parts[open_index]
        .placeholder()
        .expect("a trait is a placeholder");
    let elements = list_xml_elements(&parts[open_index..close_index]);
    for rule in config.expand_tags {
        if !elements
            .iter()
            .any(|element| element.starts_with(&format!("<{}", rule.contains)))
        {
            continue;
        }
        if rule.only_text_in_tag {
            let (Some(left), Some(right)) = (
                get_left_or_null(parts, rule.contains, open_index),
                get_right_or_null(parts, rule.contains, close_index),
            ) else {
                continue;
            };
            let sub = &parts[left..right];
            let chunks = chunk_by(sub, |part| {
                if is_tag_start(part, rule.contains) {
                    Boundary::Start
                } else if is_tag_end(part, rule.contains) {
                    Boundary::End
                } else {
                    Boundary::Inside
                }
            });
            let first = chunks.first().map_or(0, |chunk| {
                chunk.iter().filter(|part| part.is_content()).count()
            });
            let last = chunks.last().map_or(0, |chunk| {
                chunk.iter().filter(|part| part.is_content()).count()
            });
            if first != 1 || last != 1 {
                continue;
            }
        }
        let structured = structured_positions(&elements);
        let mut open_count = 0isize;
        for (name, position) in structured {
            if name == rule.expand {
                match position {
                    Position::Start => open_count += 1,
                    Position::End => open_count -= 1,
                    Position::SelfClosing => {}
                }
            }
        }
        if open_count != 0 {
            return Err(vec![errors::loop_position(&open.value)]);
        }
        return Ok(Some(rule.expand.to_string()));
    }
    if !check_start_end(&elements) {
        return Err(vec![errors::loop_position(&open.value)]);
    }
    Ok(None)
}

/// The markup between two tags, with a matched pair removed and a crossed pair kept.
fn list_xml_elements(parts: &[Part]) -> Vec<String> {
    let mut result: Vec<String> = Vec::new();
    for part in parts {
        let Some(tag) = part.tag() else {
            continue;
        };
        match tag.position {
            Position::End => {
                let closes_last = result
                    .last()
                    .is_some_and(|last| last[1..].starts_with(&tag.name));
                if closes_last {
                    result.pop();
                } else {
                    result.push(tag.value.clone());
                }
            }
            Position::Start => result.push(tag.value.clone()),
            Position::SelfClosing => {}
        }
    }
    result
}

fn structured_positions(elements: &[String]) -> Vec<(String, Position)> {
    elements
        .iter()
        .map(|element| {
            let trimmed = element.trim_start();
            let position = if trimmed.starts_with("</") {
                Position::End
            } else {
                Position::Start
            };
            (tag_name(element), position)
        })
        .collect()
}

fn check_start_end(elements: &[String]) -> bool {
    if elements.len() % 2 == 1 {
        return false;
    }
    for index in 0..elements.len() / 2 {
        if tag_name(&elements[index]) != tag_name(&elements[elements.len() - index - 1]) {
            return false;
        }
    }
    true
}

fn get_right_or_null(parts: &[Part], elements: &str, index: usize) -> Option<usize> {
    let mut level = 1;
    for (offset, part) in parts.iter().enumerate().skip(index) {
        let text = part.text();
        if parts::is_ending(text, elements) {
            level -= 1;
        }
        if is_starting(text, elements) {
            level += 1;
        }
        if level == 0 {
            return Some(offset);
        }
    }
    None
}

fn get_left_or_null(parts: &[Part], elements: &str, index: usize) -> Option<usize> {
    let mut level = 1;
    for offset in (0..=index).rev() {
        let text = parts[offset].text();
        if is_starting(text, elements) {
            level -= 1;
        }
        if parts::is_ending(text, elements) {
            level += 1;
        }
        if level == 0 {
            return Some(offset);
        }
    }
    None
}

// ── the raw-XML module's postparse ──────────────────────────────────────────────────────────────

/// Replace each raw tag with the paragraph it stands in, so the value replaces that paragraph.
///
/// The reference finds every raw tag's paragraph before it expands any of them and collects each
/// misuse as its own error, so a part holding several misused raw tags answers every sentence
/// rather than the first.
fn raw_xml(mut parts: Vec<Part>, ctx: &mut Context) -> (Vec<Part>, Vec<TemplateError>) {
    let raw_tag = ctx.config.raw_tag;
    let mut limits: Vec<(usize, usize, usize)> = Vec::new();
    let mut errors = Vec::new();
    for (index, part) in parts.iter().enumerate() {
        let Some(placeholder) = part.placeholder().filter(|placeholder| {
            matches!(placeholder.kind, Kind::Raw)
                && !placeholder.expanded
                && placeholder.sub.is_none()
        }) else {
            continue;
        };
        let (Some(left), Some(right)) = (
            get_left_or_null(&parts, raw_tag, index),
            get_right_or_null(&parts, raw_tag, index),
        ) else {
            errors.push(errors::raw_not_in_paragraph(placeholder));
            continue;
        };
        limits.push((left, right, index));
    }
    // The reference orders its limits by their left edge, the earlier tag first within one edge,
    // and skips a limit that starts inside an earlier one: two raw tags in one paragraph answer one
    // sentence, and the surviving paragraphs are disjoint.
    limits.sort_by(|left, right| {
        left.0
            .cmp(&right.0)
            .then_with(|| parts[left.2].l_index().cmp(&parts[right.2].l_index()))
    });
    // The reference's running right edge: the furthest edge seen before this limit, zero standing
    // in for "none yet". A limit that starts inside an earlier one is skipped — but its edge still
    // counts for the next one — and the surviving paragraphs are disjoint.
    let mut max_right = 0;
    let mut expand = Vec::new();
    for (left, right, index) in limits {
        let nested = left < max_right;
        max_right = max_right.max(right);
        if nested {
            continue;
        }
        let placeholder = parts[index]
            .placeholder()
            .expect("the index names a raw tag");
        if (left + 1..right).any(|offset| offset != index && parts[offset].is_content()) {
            errors.push(errors::raw_not_alone(&placeholder.value));
        } else {
            expand.push((left, right, index));
        }
    }
    // The surviving paragraphs are disjoint, so replacing the rightmost first keeps every earlier
    // limit's indices valid.
    for (left, right, index) in expand.into_iter().rev() {
        let mut placeholder = parts[index]
            .placeholder()
            .cloned()
            .expect("the index names a raw tag");
        placeholder.expanded = true;
        parts.splice(left..=right, [Part::Placeholder(placeholder)]);
    }
    (parts, errors)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn paragraph(value: &str, position: Position, l_index: usize) -> Part {
        Part::Tag(parts::Tag {
            name: "w:p".to_string(),
            value: value.to_string(),
            position,
            text: false,
            l_index,
        })
    }

    #[test]
    fn a_paragraph_nested_in_another_is_not_a_chunk_boundary() {
        let parts = [
            paragraph("<w:p>", Position::Start, 0),
            paragraph("<w:p>", Position::Start, 1),
            paragraph("<w:p/>", Position::SelfClosing, 2),
            paragraph("</w:p>", Position::End, 3),
            paragraph("</w:p>", Position::End, 4),
        ];
        let mut level = 0isize;
        let chunks = chunk_by(&parts, |part| paragraph_boundary(part, &mut level));
        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0].len(), 5);
    }
}

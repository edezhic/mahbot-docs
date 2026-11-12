//! Rendering a postparsed part: plain values, sections, raw XML, line breaks, and the repair the
//! reference applies to an empty table, row or cell.

use std::borrow::Cow;
use std::sync::LazyLock;

use regex::Regex;
use serde_json::Value;

use super::Answer;
use super::content::{self, Config};
use super::errors::{self, TemplateError};
use super::parts::{self, Kind, Part, Placeholder, Position};
use crate::vocabulary::Family;
use crate::writing::{interpolated, scalar_value, xml};

/// The state one part's render carries: the run whose properties a line break repeats, the section
/// properties a next-page section needs, and the failures the render collected.
pub(super) struct State<'a> {
    pub config: &'static Config,
    pub linebreaks: bool,
    pub recorded_run: String,
    pub record_run: bool,
    pub sects: &'a [Vec<Part>],
    pub content_type: &'a str,
    /// The failures this part's render collected, in the order the reference collects them.
    pub errors: Vec<TemplateError>,
}

impl<'a> State<'a> {
    pub fn new(
        config: &'static Config,
        linebreaks: bool,
        sects: &'a [Vec<Part>],
        content_type: &'a str,
    ) -> Self {
        Self {
            config,
            linebreaks,
            recorded_run: String::new(),
            record_run: false,
            sects,
            content_type,
            errors: Vec::new(),
        }
    }
}

/// The scope a fill reads names from: the request's `values` in whatever JSON shape it has. The
/// reference hands that value straight to the template library, whose lookup is an own-property
/// read on it — an object's members, a string's characters and length, an array's elements and
/// length, and nothing for a number or a boolean. `null` and an absent value fault before this.
pub(super) struct Scope<'a>(&'a Value);

impl<'a> Scope<'a> {
    pub fn new(value: &'a Value) -> Self {
        Self(value)
    }

    /// The first name `Object.entries` would visit whose value is not a scalar, if any.
    pub fn non_scalar(&self) -> Option<String> {
        match self.0 {
            Value::Object(map) => map
                .iter()
                .find(|(_, value)| !scalar_value(value))
                .map(|(name, _)| name.clone()),
            Value::Array(items) => items
                .iter()
                .position(|value| !scalar_value(value))
                .map(|index| index.to_string()),
            _ => None,
        }
    }

    /// The value `name` names, as the reference's own-property read finds it: an object's member is
    /// borrowed, while a string's code unit and a length are computed here.
    fn get(&self, name: &str) -> Option<Cow<'a, Value>> {
        match self.0 {
            Value::Object(map) => map.get(name).map(Cow::Borrowed),
            Value::String(text) => string_member(text, name).map(Cow::Owned),
            Value::Array(items) => array_member(items, name).map(Cow::Owned),
            _ => None,
        }
    }

    /// The scope itself, which `{.}` names.
    fn whole(&self) -> &'a Value {
        self.0
    }
}

/// A canonical array index: JavaScript reads `"1"` as an index but `"01"` as a plain name.
fn index_of(name: &str) -> Option<usize> {
    let index: usize = name.parse().ok()?;
    (index.to_string() == name).then_some(index)
}

/// A string's own members: its UTF-16 code units by index and its `length`.
///
/// An index that lands on half a surrogate pair is lossy: JavaScript yields the lone code unit, and
/// a Rust string cannot hold one, so it becomes U+FFFD — the same limit `errors::first_characters`
/// records for a quoted context.
fn string_member(text: &str, name: &str) -> Option<Value> {
    if name == "length" {
        return Some(Value::from(text.encode_utf16().count()));
    }
    let unit = text.encode_utf16().nth(index_of(name)?)?;
    Some(Value::String(String::from_utf16_lossy(&[unit])))
}

/// An array's own members: its elements by index and its `length`.
fn array_member(items: &[Value], name: &str) -> Option<Value> {
    if name == "length" {
        return Some(Value::from(items.len()));
    }
    items.get(index_of(name)?).cloned()
}

/// What a name resolves to in the scope: the scope's own member, or the scope itself, which `{.}`
/// names.
enum Resolved<'a> {
    Missing,
    Value(Cow<'a, Value>),
}

/// Render one postparsed part into the strings the part's text is joined from.
pub(super) fn render(
    parts: &[Part],
    scope: &Scope<'_>,
    state: &mut State<'_>,
    answer: &mut Answer,
) -> Vec<String> {
    let mut out = Vec::with_capacity(parts.len());
    for part in parts {
        if state.linebreaks {
            record_runs(part, state);
        }
        match part {
            Part::Tag(tag) => out.push(tag.value.clone()),
            Part::Text { value, .. } => out.push(value.clone()),
            Part::Placeholder(placeholder) => {
                render_placeholder(placeholder, scope, state, answer, &mut out);
            }
        }
    }
    out
}

fn render_placeholder(
    placeholder: &Placeholder,
    scope: &Scope<'_>,
    state: &mut State<'_>,
    answer: &mut Answer,
    out: &mut Vec<String>,
) {
    match &placeholder.kind {
        Kind::Plain => {
            let text = value_text(&resolve(scope, &placeholder.value), placeholder, answer);
            if has_corrupt_characters(&text) {
                state
                    .errors
                    .push(errors::corrupt_characters(&placeholder.value));
                return;
            }
            if state.linebreaks {
                out.extend(render_line_breaks(&text, state));
            } else {
                out.push(xml::xml_escape(&text));
            }
        }
        Kind::Raw => match resolve(scope, &placeholder.value) {
            // The null getter's own text is a string and is written out as the raw value.
            Resolved::Missing => {
                record_missing(answer, &placeholder.value);
                out.push(parts::tag_text(&placeholder.value));
            }
            Resolved::Value(value) => render_raw(&value, placeholder, state, out),
        },
        Kind::Open { inverted, .. } => {
            let resolved = resolve(scope, &placeholder.value);
            if matches!(resolved, Resolved::Missing) {
                record_missing(answer, &placeholder.value);
            }
            let truthy = truthy(&resolved);
            let render_body = if truthy { !inverted } else { *inverted };
            let inside_paragraph_loop = inside_paragraph_loop(placeholder);
            if render_body {
                let Some(sub) = &placeholder.sub else {
                    return;
                };
                let mut pieces = render(sub, scope, state, answer);
                if placeholder.page_break && inside_paragraph_loop {
                    add_page_break_at_end(&mut pieces);
                }
                if let Some(index) = placeholder.next_page
                    && let Some(sect) = state.sects.get(index)
                {
                    add_section_before(&mut pieces, sect);
                    add_page_break_at_end(&mut pieces);
                }
                if placeholder.page_break_beginning && inside_paragraph_loop {
                    pieces.insert(0, PAGE_BREAK_PARAGRAPH.to_string());
                }
                out.extend(join_uncorrupt(pieces, state.config, state.content_type));
            } else if let Some(sect) = &placeholder.last_paragraph_sect_pr {
                if placeholder.paragraph_loop {
                    out.push(format!("<w:p><w:pPr>{sect}</w:pPr></w:p>"));
                } else {
                    out.push(format!(
                        "</w:t></w:r></w:p><w:p><w:pPr>{sect}</w:pPr><w:r><w:t>"
                    ));
                }
            } else if placeholder.page_break && inside_paragraph_loop {
                out.push(PAGE_BREAK_PARAGRAPH.to_string());
            } else {
                out.push(String::new());
            }
        }
        Kind::Close => {}
    }
}

/// The value a plain tag renders, with a missing name rendered as the reference's null getter.
fn value_text(resolved: &Resolved<'_>, placeholder: &Placeholder, answer: &mut Answer) -> String {
    let Resolved::Value(value) = resolved else {
        record_missing(answer, &placeholder.value);
        return parts::tag_text(&placeholder.value);
    };
    interpolated(Some(value.as_ref()))
}

/// A raw tag's value: the empty string for a falsy value, a string as it stands, and a failure for
/// anything else, as the reference's raw module renders it.
fn render_raw(
    value: &Value,
    placeholder: &Placeholder,
    state: &mut State<'_>,
    out: &mut Vec<String>,
) {
    if is_falsy(value) {
        out.push(String::new());
    } else if let Value::String(text) = value {
        out.push(text.clone());
    } else {
        state.errors.push(errors::raw_not_text(&placeholder.raw));
    }
}

fn resolve<'a>(scope: &Scope<'a>, name: &str) -> Resolved<'a> {
    if name == "." {
        return Resolved::Value(Cow::Borrowed(scope.whole()));
    }
    // A null value is refused before the render: `scalar_value` accepts text, numbers and booleans
    // only, so a name the scope states always resolves to a value.
    match scope.get(name) {
        None => Resolved::Missing,
        Some(value) => Resolved::Value(value),
    }
}

fn truthy(resolved: &Resolved<'_>) -> bool {
    match resolved {
        // The null getter's own text is never empty: it is the tag between braces.
        Resolved::Missing => true,
        Resolved::Value(value) => !is_falsy(value),
    }
}

fn is_falsy(value: &Value) -> bool {
    match value {
        Value::Null => true,
        Value::Bool(boolean) => !boolean,
        Value::Number(number) => number.as_f64() == Some(0.0),
        Value::String(text) => text.is_empty(),
        Value::Array(values) => values.is_empty(),
        Value::Object(_) => false,
    }
}

fn record_missing(answer: &mut Answer, name: &str) {
    if answer.seen_missing.insert(name.to_string()) {
        answer.missing.push(name.to_string());
    }
}

/// Whether the first part of a section's body that is not text is something other than a run's
/// text element: a paragraph loop's body starts with its paragraph, an inline section's does not.
fn inside_paragraph_loop(placeholder: &Placeholder) -> bool {
    let Some(sub) = &placeholder.sub else {
        return false;
    };
    for part in sub {
        match part {
            Part::Text { .. } => {}
            Part::Tag(tag) => return tag.name != "w:t",
            Part::Placeholder(_) => return true,
        }
    }
    false
}

/// The control characters XML cannot carry.
fn has_corrupt_characters(text: &str) -> bool {
    text.chars().any(
        |character| matches!(character, '\u{0}'..='\u{8}' | '\u{b}' | '\u{c}' | '\u{e}'..='\u{1f}'),
    )
}

/// A value's line breaks as the run breaks the reference writes for them.
fn render_line_breaks(value: &str, state: &State<'_>) -> Vec<String> {
    let config = state.config;
    let mut result = Vec::new();
    let lines: Vec<&str> = value.split('\n').collect();
    for (index, line) in lines.iter().enumerate() {
        result.push(xml::xml_escape(line));
        if index + 1 < lines.len() {
            result.push(format!(
                "</{text}></{run}>{br}<{run}>{recorded}<{text}{suffix}>",
                text = config.text_element,
                run = config.run_tag,
                br = config.break_tag,
                recorded = state.recorded_run,
                suffix = config.linebreak_suffix,
            ));
        }
    }
    result
}

/// The run properties the reference repeats across a line break.
fn record_runs(part: &Part, state: &mut State<'_>) {
    let config = state.config;
    match part {
        Part::Tag(tag) if tag.name == config.run_tag => state.recorded_run.clear(),
        Part::Tag(tag) if tag.name == config.run_properties_tag => {
            if tag.position == Position::Start {
                state.record_run = true;
                state.recorded_run.push_str(&tag.value);
            }
            if matches!(tag.position, Position::End | Position::SelfClosing) {
                state.recorded_run.push_str(&tag.value);
                state.record_run = false;
            }
        }
        other if state.record_run => state.recorded_run.push_str(other.text()),
        _ => {}
    }
}

const PAGE_BREAK_PARAGRAPH: &str = "<w:p><w:r><w:br w:type=\"page\"/></w:r></w:p>";

fn add_page_break_at_end(parts: &mut Vec<String>) {
    if parts.last().is_some_and(|part| part == "</w:p>") {
        parts.insert(
            parts.len() - 1,
            "<w:r><w:br w:type=\"page\"/></w:r>".to_string(),
        );
    } else {
        parts.push(PAGE_BREAK_PARAGRAPH.to_string());
    }
}

fn add_section_before(parts: &mut Vec<String>, sect: &[Part]) {
    let body: String = sect.iter().map(Part::text).collect();
    parts.insert(0, format!("<w:p><w:pPr>{body}</w:pPr></w:p>"));
}

// ── the reference's render repair ──────────────────────────────────────────────────────────────

/// The strings a render produced, with the reference's repair of an empty table, row or cell.
fn join_uncorrupt(mut parts: Vec<String>, config: &Config, content_type: &str) -> Vec<String> {
    if config.family == Family::Docx && content::is_docx_type(content_type) {
        parts = add_empty_paragraph_after_table(parts);
    }
    for rule in config.should_contain {
        let mut collecting: Option<usize> = None;
        let mut collected: Vec<String> = Vec::new();
        for index in 0..parts.len() {
            let part = parts[index].clone();
            if let Some(start) = collecting {
                if parts::is_ending(&part, rule.tag) {
                    collecting = None;
                    if let Some(parent) = rule.drop_parent {
                        let mut from = start;
                        while from > 0 && !parts::is_starting(&parts[from], parent) {
                            from -= 1;
                        }
                        for part in &mut parts[from..] {
                            let last = parts::is_ending(part, parent);
                            part.clear();
                            if last {
                                break;
                            }
                        }
                    } else {
                        for part in &mut parts[start..=index] {
                            part.clear();
                        }
                        if !rule.drop {
                            parts[index] = format!("{}{}{}", collected.concat(), rule.value, part);
                        }
                    }
                    collected.clear();
                } else {
                    if !rule.drop && rule.drop_parent.is_none() {
                        collected.push(part.clone());
                    }
                    if rule
                        .contains
                        .iter()
                        .any(|name| parts::is_starting(&part, name))
                    {
                        collecting = None;
                        collected.clear();
                    }
                }
            }
            if collecting.is_none()
                && parts::is_starting(&part, rule.tag)
                && part.len() > 1
                && !part[1..].contains('<')
            {
                if part.as_bytes().get(part.len().wrapping_sub(2)) == Some(&b'/') {
                    parts[index].clear();
                } else {
                    collecting = Some(index);
                    collected = if rule.drop || rule.drop_parent.is_some() {
                        Vec::new()
                    } else {
                        vec![part.clone()]
                    };
                }
            }
        }
    }
    parts
}

/// The reference's repair of a table that a render left followed by something a table may not be
/// followed by: an empty paragraph is placed between them.
fn add_empty_paragraph_after_table(parts: Vec<String>) -> Vec<String> {
    let mut parts = parts;
    let mut last_non_empty = String::new();
    for part in &mut parts {
        if parts::is_whitespace(part) || part.starts_with("<w:bookmarkEnd") {
            continue;
        }
        if last_non_empty.ends_with("</w:tbl>")
            && !part.starts_with("<w:p")
            && !part.starts_with("<w:tbl")
            && !part.starts_with("<w:sectPr")
            && !part.starts_with("</w:ftr>")
            && !part.starts_with("</w:hdr>")
        {
            part.insert_str(0, "<w:p/>");
        }
        last_non_empty.clone_from(part);
    }
    parts
}

/// The reference's final pass over a part's rendered strings: the Word space-preserve module turns
/// a text element it emptied into a self-closing one, then every module's join runs.
pub(super) fn postrender(parts: Vec<String>, config: &Config, content_type: &str) -> Vec<String> {
    let parts = if config.space_preserve {
        empty_text_elements(parts)
    } else {
        parts
    };
    join_uncorrupt(parts, config, content_type)
}

/// `<w:t xml:space="preserve"></w:t>` as `<w:t/>`, the shape the reference writes for a text
/// element a render emptied.
fn empty_text_elements(mut parts: Vec<String>) -> Vec<String> {
    const OPEN: &str = "<w:t xml:space=\"preserve\">";
    const CLOSE: &str = "</w:t>";
    let mut last_non_empty = 0;
    let mut ends_with_open = false;
    for index in 0..parts.len() {
        if parts[index].is_empty() {
            continue;
        }
        let mut part = parts[index].clone();
        if ends_with_open && part.starts_with(CLOSE) {
            if let Some(position) = parts[last_non_empty].rfind(OPEN) {
                parts[last_non_empty].truncate(position);
                parts[last_non_empty].push_str("<w:t/>");
            }
            part = part[CLOSE.len()..].to_string();
            parts[index].clone_from(&part);
        }
        ends_with_open = part.ends_with(OPEN);
        last_non_empty = index;
    }
    parts
}

// ── the kit's own presenter-notes pass ─────────────────────────────────────────────────────────

/// The kit's plainer fill of a presenter-notes page: only `{name}` is understood, so a section tag
/// is a literal name.
pub(super) fn fill_notes(text: &str, scope: &Scope<'_>, answer: &mut Answer) -> String {
    let element = xml::element_pattern("a:p");
    let runs = &RUN_PATTERN;
    let placeholder = &PLACEHOLDER_PATTERN;
    element
        .replace_all(text, |captures: &regex::Captures<'_>| {
            let whole = captures.get(0).map_or("", |found| found.as_str());
            let mut joined = String::new();
            for run in runs.captures_iter(whole) {
                joined.push_str(run.get(1).map_or("", |found| found.as_str()));
            }
            let joined = xml::xml_unescape(&joined);
            if !placeholder.is_match(&joined) {
                return whole.to_string();
            }
            let substituted = placeholder
                .replace_all(&joined, |capture: &regex::Captures<'_>| {
                    let name = capture.get(1).map_or("", |found| found.as_str());
                    answer.tags.insert(name.to_string());
                    if let Some(value) = scope.get(name) {
                        interpolated(Some(value.as_ref()))
                    } else {
                        record_missing(answer, name);
                        capture
                            .get(0)
                            .map_or("", |found| found.as_str())
                            .to_string()
                    }
                })
                .into_owned();
            let mut first = true;
            runs.replace_all(whole, |_: &regex::Captures<'_>| {
                if first {
                    first = false;
                    format!("<a:t>{}</a:t>", xml::xml_escape(&substituted))
                } else {
                    "<a:t></a:t>".to_string()
                }
            })
            .into_owned()
        })
        .into_owned()
}

/// The run element of a presenter-notes page, capturing its text.
static RUN_PATTERN: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(&format!(
        "{}([\\s\\S]*?){}",
        xml::open_tag_with("a:t", xml::TAG_MIDDLE),
        xml::close_tag("a:t"),
    ))
    .expect("the run pattern is built from fixed words")
});

/// A plain `{name}` tag, the only kind the presenter-notes pass understands.
static PLACEHOLDER_PATTERN: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\{([^{}]+)\}").expect("the tag pattern is a fixed pattern"));

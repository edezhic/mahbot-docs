//! A mark's own sentence, as the pattern a test matches one whole answer line with.
//!
//! Both ledgers — the reading's and the rendering's — check that a mark recorded unexercised is not
//! printed by any corpus document, and each needs the same reading of a mark's own words: one home,
//! so the two checks cannot disagree about what a sentence is.

/// The shortest literal text a mark needs for its sentence to be recognisable: a value word such as
/// `TRUE` or `row` names a value rather than a sentence, and a pattern over it would match anything.
pub const RECOGNISABLE: usize = 16;

/// A mark's sentence as a pattern covering one whole line, with each `{placeholder}` standing for any
/// text — empty included, because a plural's `{s}` stands for nothing when there is one — or `None`
/// when the mark's own words are too short to recognise.
pub fn sentence(text: &str) -> Option<regex::Regex> {
    let mut pattern = String::from("^");
    let mut literal = 0;
    let mut rest = text;
    while let Some(start) = rest.find('{') {
        let (before, after) = rest.split_at(start);
        pattern.push_str(&regex::escape(before));
        literal += before.len();
        let end = after.find('}')?;
        pattern.push_str(".*?");
        rest = &after[end + 1..];
    }
    pattern.push_str(&regex::escape(rest));
    literal += rest.len();
    pattern.push('$');
    (literal >= RECOGNISABLE).then(|| regex::Regex::new(&pattern).expect("a mark's own words"))
}

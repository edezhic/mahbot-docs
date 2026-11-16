//! Where a line may break inside text that has no spaces in it: the port of the pinned library's
//! line-break rules.
//!
//! A line breaks after spaces and tabs, which the fill places as tokens of their own, and at the
//! opportunities this module finds inside a word: after a zero-width space, after a dash that is not
//! part of a number, and around Chinese and Japanese characters, which Word breaks between whenever
//! the document's compatibility mode is at or past Word 2010's. A created document states Word 2013's
//! mode, so those breaks are on.
//!
//! The character classes are the reference's own regular expressions, enumerated as code-point ranges
//! by the derivation command (`assets/docx-layout/unicode.json`) rather than restated here: `\p{M}`, a
//! mark that belongs to the character before it, and the East Asian and Hangul script classes.
//!
//! # What a created document does not reach
//!
//! Word's kinsoku lists — the characters that may not start or end a line of Chinese or Japanese — are
//! gated on the run's East Asian language, and a created document's runs state none, so the reference
//! resolves every character to its empty list and the veto never fires. The `anywhere` rule, which
//! breaks inside a word whose run is East Asian when the paragraph turns word wrapping off, is
//! unreachable for the same reason: a created document states no `w:wordWrap`.

use std::collections::BTreeSet;
use std::sync::LazyLock;

/// A set of code-point ranges, in ascending order.
struct Ranges(Vec<(u32, u32)>);

impl Ranges {
    fn contains(&self, character: char) -> bool {
        let code = character as u32;
        self.0
            .binary_search_by(|(first, last)| {
                if code < *first {
                    std::cmp::Ordering::Greater
                } else if code > *last {
                    std::cmp::Ordering::Less
                } else {
                    std::cmp::Ordering::Equal
                }
            })
            .is_ok()
    }
}

#[derive(serde::Deserialize)]
struct UnicodeData {
    marks: Vec<[u32; 2]>,
    east_asian: Vec<[u32; 2]>,
    hangul: Vec<[u32; 2]>,
}

static UNICODE: LazyLock<UnicodeData> = LazyLock::new(|| {
    serde_json::from_str(include_str!("../../../../assets/docx-layout/unicode.json"))
        .expect("the character classes are the derivation command's own output")
});

static MARKS: LazyLock<Ranges> = LazyLock::new(|| ranges(&UNICODE.marks));
static EAST_ASIAN: LazyLock<Ranges> = LazyLock::new(|| ranges(&UNICODE.east_asian));
static HANGUL: LazyLock<Ranges> = LazyLock::new(|| ranges(&UNICODE.hangul));

fn ranges(list: &[[u32; 2]]) -> Ranges {
    Ranges(list.iter().map(|[first, last]| (*first, *last)).collect())
}

/// Whether a character is a space the fill places as a token of its own: the five the reference
/// breaks words at.
pub(crate) fn is_space(character: char) -> bool {
    matches!(
        character,
        ' ' | '\u{2002}' | '\u{2003}' | '\u{2005}' | '\u{3000}'
    )
}

/// Whether a character belongs to the one before it, so a line never breaks between them.
fn is_extender(character: char) -> bool {
    let code = character as u32;
    MARKS.contains(character)
        || character == '\u{200D}'
        || (0xFE00..=0xFE0F).contains(&code)
        || (0x1F3FB..=0x1F3FF).contains(&code)
        || (0xE0100..=0xE01EF).contains(&code)
}

/// Whether a character joins the one after it to the one before it.
fn joins_next(character: char) -> bool {
    character == '\u{200D}'
}

/// Whether a character glues its neighbours together, so a line never breaks beside it.
fn is_glue(character: char) -> bool {
    matches!(
        character,
        '\u{00A0}' | '\u{202F}' | '\u{2007}' | '\u{2060}' | '\u{FEFF}'
    )
}

/// Whether a character is a dash a line may break after.
fn is_dash(character: char) -> bool {
    matches!(character, '-' | '\u{2010}' | '\u{2013}' | '\u{2014}')
}

/// Whether a character is Chinese, Japanese or Korean, or East Asian punctuation: the reference's
/// `EAST_ASIAN` class, which is also the class that decides which of a run's characters Word draws
/// in the run's East Asian face.
pub(crate) fn is_east_asian(character: char) -> bool {
    EAST_ASIAN.contains(character)
}

/// Whether a line can break before and after a character: Chinese and Japanese characters, and East
/// Asian punctuation, but not Korean.
fn breaks_around(character: char) -> bool {
    EAST_ASIAN.contains(character) && !HANGUL.contains(character)
}

/// The code-point indices a new token may start at: the reference's `findLineBreaks`, for a document
/// whose runs state no language (so every kinsoku list is empty) and whose paragraph does not turn
/// word wrapping off. `ideographs` is whether the document's compatibility mode breaks between
/// Chinese and Japanese characters, which the settings part states.
pub(crate) fn breaks(characters: &[char], ideographs: bool) -> BTreeSet<usize> {
    let mut found = BTreeSet::new();
    // The reference short-circuits on text with nothing to break inside: no character above U+02FF
    // and no hyphen.
    if !characters.iter().any(|character| (*character as u32) > 767) && !characters.contains(&'-') {
        return found;
    }
    for index in 1..characters.len() {
        let before = characters[index - 1];
        let after = characters[index];
        if is_extender(after) || joins_next(before) || is_glue(before) || is_glue(after) {
            continue;
        }
        let permitted = before == '\u{200B}'
            || (is_dash(before) && !after.is_ascii_digit() && after != '-')
            || (ideographs && (breaks_around(before) || breaks_around(after)));
        if permitted {
            found.insert(index);
        }
    }
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    fn breaks_of(text: &str) -> Vec<usize> {
        breaks(&text.chars().collect::<Vec<_>>(), true)
            .into_iter()
            .collect()
    }

    #[test]
    fn chinese_and_japanese_characters_break_between_each_other() {
        // A created document's compatibility mode is Word 2013's, so ideograph breaking is on.
        assert_eq!(breaks_of("東アジア"), vec![1, 2, 3]);
        // Korean does not break between syllables.
        assert_eq!(breaks_of("한글구조"), Vec::<usize>::new());
    }

    #[test]
    fn a_dash_breaks_after_itself_unless_it_is_part_of_a_number() {
        assert_eq!(breaks_of("well-known"), vec![5]);
        assert_eq!(breaks_of("1-2"), Vec::<usize>::new());
    }

    #[test]
    fn a_zero_width_space_breaks_and_a_glue_does_not() {
        assert_eq!(breaks_of("one\u{200B}two"), vec![4]);
        assert_eq!(breaks_of("one\u{00A0}two"), Vec::<usize>::new());
    }

    #[test]
    fn a_mark_belongs_to_the_character_before_it() {
        // The mark takes no break opportunity of its own, and neither does the character before it.
        assert_eq!(breaks_of("a\u{0301}b"), Vec::<usize>::new());
    }
}

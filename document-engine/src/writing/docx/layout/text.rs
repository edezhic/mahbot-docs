//! How wide and how tall text is: the port of the reference's own measurer.
//!
//! The widths are the pinned library's tables, shipped as data beside the crate
//! (`assets/docx-layout/`), decoded exactly as the library decodes them: two base-64 digits a
//! thousandth of an em for the core table, three digits a tenth of a thousandth for the supplementary
//! one, `!` for a width the table does not state, `*` and two digits for a run of the value before
//! it, `=` for the width of the character's NFD base, and `~` and a digit for a width Word takes from
//! another face of the table.
//!
//! # What a created document reaches
//!
//! A created document's runs name no font and no kerning, so every run measures in the default face
//! — Times New Roman at ten points, which is what the reference's measurer falls back to when neither
//! the run nor the style chain states one — and the kerning and ligature tables are never consulted:
//! `measureTextWidthAsDrawn` is `measureTextWidth` whenever the run asks for neither. Arabic is not
//! gated the same way: its letters are joined whenever the face has an Arabic table, which Times New
//! Roman does, so a joined run is measured narrower than the sum of its isolated letters.
//!
//! # Not ported
//!
//! The cloud-font list (`office-cloud-fonts.ts`), the font-file measurer and the kerning and ligature
//! tables are not read, because a created document cannot reach them: it names no font of its own,
//! embeds none, and states no kerning or ligature setting. The East Asian fallback faces' metrics
//! are shipped and used, since a run that names no East Asian face is measured in the reference's
//! fallback. A character the tables state no width for is measured as the average letter of the face;
//! a character the tables mark unknown — or a joined letter whose face has no Arabic table — is
//! reported through [`unknown_character`], which the layout turns into a stop.

use std::collections::HashMap;
use std::sync::LazyLock;

use serde::Deserialize;

/// The face a run with no font of its own is measured in: the reference's default.
const DEFAULT_FONT: &str = "Times New Roman";

/// The face the layout draws East Asian text in when the run names no East Asian face of its own: the
/// reference's fallback.
const FALLBACK_EAST_ASIAN_FONT: &str = "MS Mincho";

/// The size a run with no size of its own is measured at, in points.
const DEFAULT_SIZE: f64 = 10.0;

/// The thousandths of an em the width tables state a width in.
const THOUSANDTHS: f64 = 1000.0;

/// The distance between automatic tab stops, in points, from the layout constants data.
pub(crate) fn tab_stop() -> f64 {
    super::constants::value("TAB_STOP").expect("the layout data states the tab stop")
}

/// The digits the width tables are written in.
const DIGITS: &[u8] = b"0123456789abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ+/";

/// A run's font, as the measurer reads it: the face, its size, its face style, and the two settings
/// that change how wide its characters are drawn.
#[derive(Clone, Debug, Default)]
pub(crate) struct Font {
    /// The face's name. An absent name is the default face.
    pub(crate) name: Option<String>,
    /// The size in points. An absent size is the default.
    pub(crate) size: Option<f64>,
    pub(crate) bold: bool,
    pub(crate) italic: bool,
    /// Letter spacing, in points, added to every character's own advance.
    pub(crate) character_spacing: f64,
    /// A character-scale percentage, as `w:w` states it: 100 draws text at its size.
    pub(crate) scale: f64,
    /// Whether the face is one of the East Asian faces, whose characters are an em or half an em wide
    /// and whose line metrics are their own.
    pub(crate) monospaced: bool,
}

impl Font {
    /// The face's name, or the default one.
    fn face(&self) -> &str {
        self.name.as_deref().unwrap_or(DEFAULT_FONT)
    }

    /// The size the run is measured at.
    pub(crate) fn size(&self) -> f64 {
        self.size.unwrap_or(DEFAULT_SIZE)
    }

    /// The character scale, as the fraction the reference multiplies by.
    fn scale_fraction(&self) -> f64 {
        if self.scale == 0.0 { 100.0 } else { self.scale }
    }
}

// ── the data ─────────────────────────────────────────────────────────────────────────────────────

/// The width tables, as the derivation command wrote them.
#[derive(Deserialize)]
struct WidthsData {
    ranges: Vec<[u32; 2]>,
    more_ranges: Vec<[u32; 2]>,
    fallback_faces: Vec<FallbackFace>,
    faces: Vec<FaceData>,
    same_widths: Vec<Alias>,
    similar_fonts: Vec<Alias>,
}

#[derive(Deserialize)]
struct FallbackFace {
    font: String,
    bold: bool,
    italic: bool,
}

#[derive(Deserialize)]
struct FaceData {
    name: String,
    line_height: f64,
    descent: f64,
    regular: String,
    #[serde(default)]
    bold: Option<String>,
    #[serde(default)]
    italic: Option<String>,
    #[serde(default)]
    bold_italic: Option<String>,
}

/// A pattern that measures as another face: the library's own list, kept as data so the two cannot
/// disagree about which fonts are made with the same widths.
#[derive(Deserialize)]
struct Alias {
    pattern: String,
    /// The pattern's regular-expression flags, which the data states and this port does not run: the
    /// name is lowercased instead, which is what `i` asks for and the only flag every alias states.
    /// Kept rather than dropped so [`Widths::new`] can refuse a flag it does not apply.
    flags: String,
    face: String,
}

/// The supplementary tables: the characters the core table does not state.
#[derive(Deserialize)]
struct MoreData {
    faces: Vec<MoreFace>,
    fallback_fonts: Vec<FallbackFont>,
}

#[derive(Deserialize)]
struct MoreFace {
    name: String,
    classes: MoreClasses,
}

#[derive(Deserialize)]
struct MoreClasses {
    regular: MoreClass,
    #[serde(default)]
    bold: Option<MoreClass>,
    #[serde(default)]
    italic: Option<MoreClass>,
    #[serde(default)]
    bold_italic: Option<MoreClass>,
}

#[derive(Deserialize)]
struct MoreClass {
    widths: String,
    fonts: String,
}

#[derive(Deserialize)]
struct FallbackFont {
    /// The face's name, which the data states and the line height does not need.
    #[expect(
        dead_code,
        reason = "a line's height is the fallback face's, not its name's"
    )]
    name: String,
    ascent: f64,
    descent: f64,
}

/// The Arabic tables: the letters and the ligatures Word joins, per face.
#[derive(Deserialize)]
struct ArabicData {
    letters: String,
    lam_alefs: String,
    faces: Vec<ArabicFace>,
}

#[derive(Deserialize)]
struct ArabicFace {
    /// The face's name, which the data states and the table is found by.
    name: String,
    regular: String,
    bold: String,
    /// The face the letters are drawn in, which the reference records and this port does not need:
    /// the widths are the table's own.
    #[serde(default)]
    #[expect(
        dead_code,
        reason = "the widths are the table's own, not the face's it is drawn in"
    )]
    drawn_in: Option<String>,
}

/// The East Asian faces the layout measures Chinese, Japanese and Korean text in.
#[derive(Deserialize)]
struct EastAsianData {
    faces: Vec<EastAsianFace>,
}

#[derive(Deserialize)]
struct EastAsianFace {
    name: String,
    #[serde(default)]
    aliases: Vec<String>,
    #[serde(rename = "lineHeight")]
    line_height: f64,
    descent: f64,
    #[serde(default)]
    monospaced: bool,
    /// The face the East Asian face's Latin letters are measured as.
    latin: String,
}

static EAST_ASIAN: LazyLock<EastAsianData> = LazyLock::new(|| {
    serde_json::from_str(include_str!(
        "../../../../assets/docx-layout/east_asian.json"
    ))
    .expect("the East Asian face table is the derivation command's own output")
});

/// The East Asian face `name` names, by any of its names, or `None` for another face.
fn east_asian_face(name: &str) -> Option<&'static EastAsianFace> {
    EAST_ASIAN.faces.iter().find(|face| {
        face.name.eq_ignore_ascii_case(name)
            || face
                .aliases
                .iter()
                .any(|alias| alias.eq_ignore_ascii_case(name))
    })
}

/// Whether a face is one Word draws Chinese, Japanese or Korean text in.
fn is_east_asian_font(name: &str) -> bool {
    if east_asian_face(name).is_some() {
        return true;
    }
    let name = name.to_lowercase();
    // The reference's own name pattern: a Chinese, Japanese or Korean character in the name, or one
    // of the families Word installs for those scripts.
    name.chars().any(|character| {
        let code = character as u32;
        (0x3040..=0x30FF).contains(&code)
            || (0x3400..=0x9FFF).contains(&code)
            || (0xAC00..=0xD7AF).contains(&code)
    }) || [
        "hiragino",
        "cjk",
        "source han",
        "pingfang",
        "songti",
        "heiti",
        "kaiti",
        "fangsong",
        "mincho",
        "mingliu",
        "simhei",
        "gungsuh",
        "nanum",
    ]
    .iter()
    .any(|needle| name.contains(needle))
}

/// The face a run's East Asian text is drawn in: the run's own East Asian face when it names one of
/// the table's, and the reference's fallback otherwise.
pub(crate) fn east_asian_font(run: &Font) -> Font {
    let name = match run.name.as_deref() {
        Some(name) if is_east_asian_font(name) => name,
        _ => FALLBACK_EAST_ASIAN_FONT,
    };
    match east_asian_face(name) {
        // A face whose characters are not all an em or half an em wide is measured as the face its
        // Latin letters are.
        Some(face) if !face.monospaced => Font {
            name: Some(face.latin.clone()),
            ..run.clone()
        },
        _ => Font {
            name: Some(name.to_string()),
            monospaced: true,
            ..run.clone()
        },
    }
}

static WIDTHS: LazyLock<Widths> = LazyLock::new(|| {
    let data: WidthsData =
        serde_json::from_str(include_str!("../../../../assets/docx-layout/widths.json"))
            .expect("the width data is the derivation command's own output");
    let more: MoreData = serde_json::from_str(include_str!(
        "../../../../assets/docx-layout/more_widths.json"
    ))
    .expect("the supplementary width data is the derivation command's own output");
    let arabic: ArabicData = serde_json::from_str(include_str!(
        "../../../../assets/docx-layout/arabic_widths.json"
    ))
    .expect("the Arabic width data is the derivation command's own output");
    Widths::new(data, more, arabic)
});

/// The decoded tables: a face's widths by code point, the supplementary ones, and the Arabic ones.
struct Widths {
    index: HashMap<u32, usize>,
    more_index: HashMap<u32, usize>,
    fallback_faces: Vec<FallbackFace>,
    faces: Vec<Face>,
    same_width_faces: Vec<Alias>,
    similar_fonts: Vec<Alias>,
    fallback_fonts: Vec<FallbackFont>,
    letters: String,
    lam_alefs: String,
}

/// One face of the core table, decoded.
struct Face {
    name: String,
    line_height: f64,
    descent: f64,
    regular: Decoded,
    bold: Decoded,
    italic: Decoded,
    bold_italic: Decoded,
    /// The supplementary tables the face states, one per style, decoded once.
    more: Option<MoreStyles>,
    /// The Arabic table the face states, plain and bold, decoded once.
    arabic_regular: Option<Vec<Option<f64>>>,
    arabic_bold: Option<Vec<Option<f64>>>,
}

/// A face's widths: one entry per code point of the core table, and which fallback face each takes
/// its width from when the table names one.
struct Decoded {
    widths: Vec<Option<f64>>,
    fallbacks: Vec<Option<usize>>,
}

/// The supplementary face a character is drawn in, and its width there.
struct MoreDecoded {
    widths: Vec<Option<f64>>,
    fonts: Vec<Option<usize>>,
}

/// A face's supplementary tables, one per face style.
struct MoreStyles {
    regular: MoreDecoded,
    bold: Option<MoreDecoded>,
    italic: Option<MoreDecoded>,
    bold_italic: Option<MoreDecoded>,
}

impl MoreStyles {
    /// The supplementary table text in `font` is drawn in, when the table holds one for its style.
    fn class(&self, font: &Font) -> Option<&MoreDecoded> {
        match (font.bold, font.italic) {
            (true, true) => self.bold_italic.as_ref(),
            (true, false) => self.bold.as_ref(),
            (false, true) => self.italic.as_ref(),
            (false, false) => Some(&self.regular),
        }
    }
}

impl Widths {
    fn new(data: WidthsData, more: MoreData, arabic: ArabicData) -> Self {
        // The port matches an alias by lowercasing the name, which is what the `i` flag asks for and
        // the only flag the data states. A data set that states another is a defect of the derivation
        // command, and a rule applied as if it had `i` would measure silently wrong, so the tables
        // refuse to build rather than mis-measure.
        for alias in data.same_widths.iter().chain(&data.similar_fonts) {
            assert_eq!(
                alias.flags, "i",
                "the width data states {} with flags the port does not apply: {}",
                alias.pattern, alias.flags
            );
        }
        let characters: Vec<u32> = data
            .ranges
            .iter()
            .flat_map(|[first, last]| *first..=*last)
            .collect();
        let index = characters
            .iter()
            .enumerate()
            .map(|(at, code)| (*code, at))
            .collect();
        let more_index = data
            .more_ranges
            .iter()
            .flat_map(|[first, last]| *first..=*last)
            .enumerate()
            .map(|(at, code)| (code, at))
            .collect();
        let faces: Vec<Face> = data
            .faces
            .into_iter()
            .map(|face| {
                // The supplementary and Arabic tables are decoded once per face rather than on every
                // measurement, which is what the reference's own lazy caches do.
                let more_table = more.faces.iter().find(|other| other.name == face.name);
                let arabic_table = arabic.faces.iter().find(|other| other.name == face.name);
                Face {
                    more: more_table.map(MoreStyles::of),
                    arabic_regular: arabic_table.map(|table| decode_arabic(&table.regular)),
                    arabic_bold: arabic_table.map(|table| decode_arabic(&table.bold)),
                    name: face.name,
                    line_height: face.line_height,
                    descent: face.descent,
                    // A face without a bold or an italic of its own is drawn with the one it has: bold
                    // italic is its bold, and an italic is the upright face slanted, as Word draws them.
                    regular: decode(&face.regular, &characters, &index),
                    bold: decode(
                        face.bold.as_ref().unwrap_or(&face.regular),
                        &characters,
                        &index,
                    ),
                    italic: decode(
                        face.italic.as_ref().unwrap_or(&face.regular),
                        &characters,
                        &index,
                    ),
                    bold_italic: decode(
                        face.bold_italic
                            .as_ref()
                            .or(face.bold.as_ref())
                            .unwrap_or(&face.regular),
                        &characters,
                        &index,
                    ),
                }
            })
            .collect();
        Self {
            index,
            more_index,
            fallback_faces: data.fallback_faces,
            faces,
            same_width_faces: data.same_widths,
            similar_fonts: data.similar_fonts,
            fallback_fonts: more.fallback_fonts,
            letters: arabic.letters,
            lam_alefs: arabic.lam_alefs,
        }
    }

    /// The face text in `font` is measured with: its own, or the one it is made with the same widths
    /// as, or the most similar one the table holds.
    fn face_of(&self, font: &Font) -> &Face {
        let name = font.face();
        let exact = alias(&self.same_width_faces, name)
            .or_else(|| {
                self.faces
                    .iter()
                    .find(|face| face.name.eq_ignore_ascii_case(name))
                    .map(|face| face.name.clone())
            })
            .or_else(|| alias(&self.similar_fonts, name))
            .unwrap_or_else(|| "Arial".to_string());
        self.faces
            .iter()
            .find(|face| face.name == exact)
            .unwrap_or(&self.faces[0])
    }

    /// The decoded face style text is drawn in.
    fn decoded(&self, font: &Font) -> &Decoded {
        let face = self.face_of(font);
        match (font.bold, font.italic) {
            (true, true) => &face.bold_italic,
            (true, false) => &face.bold,
            (false, true) => &face.italic,
            (false, false) => &face.regular,
        }
    }

    /// The supplementary face of the font, when the table holds one.
    fn more_of(&self, font: &Font) -> Option<&MoreDecoded> {
        self.face_of(font).more.as_ref()?.class(font)
    }

    /// The Arabic table of the face text is drawn in, plain or bold and never italic.
    fn arabic_of(&self, font: &Font) -> Option<&[Option<f64>]> {
        if font.italic {
            return None;
        }
        let face = self.face_of(font);
        if font.bold {
            face.arabic_bold.as_deref()
        } else {
            face.arabic_regular.as_deref()
        }
    }
}

impl MoreStyles {
    /// The supplementary tables a face states, decoded once.
    #[expect(
        clippy::cast_precision_loss,
        reason = "a decoded width is a small whole number of thousandths of an em, exactly representable"
    )]
    fn of(face: &MoreFace) -> Self {
        let class = |class: &MoreClass| MoreDecoded {
            widths: decode_tokens(&class.widths, 3, |token| {
                (digit(token.as_bytes()[0]) as f64 * 4096.0
                    + two_digits(token.as_bytes(), 1) as f64)
                    / 10.0
            }),
            fonts: decode_tokens(&class.fonts, 1, |token| {
                if token == "-" {
                    usize::MAX
                } else {
                    digit(token.as_bytes()[0])
                }
            })
            .into_iter()
            .map(|font| font.filter(|at| *at != usize::MAX))
            .collect(),
        };
        Self {
            regular: class(&face.classes.regular),
            bold: face.classes.bold.as_ref().map(class),
            italic: face.classes.italic.as_ref().map(class),
            bold_italic: face.classes.bold_italic.as_ref().map(class),
        }
    }
}

/// An Arabic table, decoded: three base-64 digits a tenth of a thousandth of an em.
#[expect(
    clippy::cast_precision_loss,
    reason = "a decoded width is a small whole number of thousandths of an em, exactly representable"
)]
fn decode_arabic(encoded: &str) -> Vec<Option<f64>> {
    decode_tokens(encoded, 3, |token| {
        (digit(token.as_bytes()[0]) as f64 * 4096.0 + two_digits(token.as_bytes(), 1) as f64) / 10.0
    })
}

/// The face a pattern names, when the font matches it.
fn alias(aliases: &[Alias], name: &str) -> Option<String> {
    aliases
        .iter()
        .find(|alias| matches(&alias.pattern, name))
        .map(|alias| alias.face.clone())
}

/// Whether `name` matches one of the reference's own patterns. The patterns are regular expressions
/// over the face's name, and each one the data states is evaluated here rather than by a
/// regular-expression engine the crate does not hold for this one use. A pattern the data states and
/// this function does not know is a defect in the data, so it panics rather than measuring as the
/// fallback face.
fn matches(pattern: &str, name: &str) -> bool {
    let name = name.to_lowercase();
    match pattern {
        "^carlito$" => name == "carlito",
        "^caladea$" => name == "caladea",
        "^(liberation sans|arimo|helvetica)$" => {
            matches!(name.as_str(), "liberation sans" | "arimo" | "helvetica")
        }
        "^(liberation serif|tinos)$" => matches!(name.as_str(), "liberation serif" | "tinos"),
        "^(liberation mono|cousine)$" => matches!(name.as_str(), "liberation mono" | "cousine"),
        "^segoe ui$" => name == "segoe ui",
        "mono|courier|code|typewriter" => ["mono", "courier", "code", "typewriter"]
            .iter()
            .any(|needle| name.contains(needle)),
        // `times|garamond|palatino|(?<!sans[ -]?)serif|roman`
        "times|garamond|palatino|(?<!sans[ -]?)serif|roman" => {
            ["times", "garamond", "palatino", "roman"]
                .iter()
                .any(|needle| name.contains(needle))
                || match name.find("serif") {
                    None => false,
                    // The lookbehind excludes "sans serif" and "sans-serif".
                    Some(at) => !name[..at].ends_with("sans ") && !name[..at].ends_with("sans-"),
                }
        }
        other => {
            panic!("the width data states an alias pattern the port does not evaluate: {other}")
        }
    }
}

/// The value of one base-64 digit of a width string. The data states base-64 digits and nothing else,
/// so a byte that is not one is a defect in the data rather than a width.
fn digit(byte: u8) -> usize {
    DIGITS
        .iter()
        .position(|digit| *digit == byte)
        .unwrap_or_else(|| panic!("a width string holds {byte:#04x}, not a base-64 digit"))
}

/// Two base-64 digits' value.
fn two_digits(encoded: &[u8], at: usize) -> usize {
    let pair = encoded
        .get(at..at + 2)
        .unwrap_or_else(|| panic!("a width string ends inside a repeat count"));
    digit(pair[0]) * 64 + digit(pair[1])
}

/// Read a token string: tokens of `size` characters, `!` for no width, and `*` with two digits
/// repeating the value before it that many more times.
fn decode_tokens<T: Clone>(encoded: &str, size: usize, read: impl Fn(&str) -> T) -> Vec<Option<T>> {
    let bytes = encoded.as_bytes();
    let mut values = Vec::new();
    let mut last: Option<T> = None;
    let mut at = 0;
    while at < bytes.len() {
        if bytes[at] == b'*' {
            let count = two_digits(bytes, at + 1);
            values.extend(std::iter::repeat_n(last.clone(), count));
            at += 3;
        } else if bytes[at] == b'!' {
            last = None;
            values.push(None);
            at += 1;
        } else {
            last = Some(read(&encoded[at..(at + size).min(encoded.len())]));
            values.push(last.clone());
            at += size;
        }
    }
    values
}

/// Read a core face's widths, with the fallback face each character takes its width from.
#[expect(
    clippy::cast_precision_loss,
    reason = "a decoded width is a small whole number of thousandths of an em, exactly representable"
)]
fn decode(encoded: &str, characters: &[u32], index: &HashMap<u32, usize>) -> Decoded {
    let bytes = encoded.as_bytes();
    let widths: Vec<Option<f64>> = Vec::new();
    let fallbacks: Vec<Option<usize>> = Vec::new();
    let mut decoded = Decoded { widths, fallbacks };
    let mut at = 0;
    let mut token = 0;
    while at < bytes.len() {
        let mut count = 1;
        if bytes[at] == b'*' {
            count = two_digits(bytes, at + 1);
            at += 3;
        } else {
            token = at;
            at += if bytes[token] == b'=' || bytes[token] == b'!' {
                1
            } else {
                2
            };
        }
        for _ in 0..count {
            let width = match bytes[token] {
                b'!' | b'~' => None,
                b'=' => {
                    // The width of the character's NFD base in the same face.
                    let code = *characters.get(decoded.widths.len()).unwrap_or(&0);
                    index
                        .get(&nfd_base(code))
                        .and_then(|at| decoded.widths.get(*at).copied().flatten())
                }
                _ => Some(two_digits(bytes, token) as f64),
            };
            decoded.widths.push(width);
            decoded.fallbacks.push(
                (bytes[token] == b'~').then(|| digit(*bytes.get(token + 1).unwrap_or(&b'0'))),
            );
        }
    }
    decoded
}

/// The code point of a character's canonical decomposition, or the character itself.
fn nfd_base(code: u32) -> u32 {
    static NFD: LazyLock<icu_normalizer::DecomposingNormalizerBorrowed<'static>> =
        LazyLock::new(icu_normalizer::DecomposingNormalizer::new_nfd);
    let Some(character) = char::from_u32(code) else {
        return code;
    };
    NFD.normalize(character.encode_utf8(&mut [0; 4]))
        .chars()
        .next()
        .map_or(code, |base| base as u32)
}

/// A character's width in thousandths of an em, from the tables.
fn character_width(face: &Decoded, more: Option<&MoreDecoded>, character: char) -> f64 {
    let code = character as u32;
    if takes_no_room(character) {
        return 0.0;
    }
    if let Some(at) = WIDTHS.index.get(&code) {
        if let Some(width) = face.widths.get(*at).copied().flatten() {
            return width;
        }
        // A width Word draws in another face of the table.
        if let Some(fallback) = face.fallbacks.get(*at).copied().flatten() {
            let other = &WIDTHS.fallback_faces[fallback];
            let font = Font {
                name: Some(other.font.clone()),
                bold: other.bold,
                italic: other.italic,
                scale: 100.0,
                ..Font::default()
            };
            let other = WIDTHS.decoded(&font);
            if let Some(width) = other.widths.get(*at).copied().flatten() {
                return width;
            }
        }
    }
    if let Some(at) = WIDTHS.more_index.get(&code)
        && let Some(width) = more.and_then(|more| more.widths.get(*at).copied().flatten())
    {
        return width;
    }
    if is_wide(code) {
        return THOUSANDTHS;
    }
    if is_half_width(code) {
        return 500.0;
    }
    // The average width of the face's lowercase letters, which is how the reference measures a
    // character no table states.
    average_letter(face)
}

/// The average width of a face's lowercase letters, in thousandths of an em.
#[expect(
    clippy::cast_precision_loss,
    reason = "a decoded width is a small whole number of thousandths of an em, exactly representable"
)]
fn average_letter(face: &Decoded) -> f64 {
    let letters: Vec<f64> = ('a'..='z')
        .filter_map(|letter| {
            WIDTHS
                .index
                .get(&(letter as u32))
                .and_then(|at| face.widths.get(*at).copied().flatten())
        })
        .collect();
    if letters.is_empty() {
        500.0
    } else {
        letters.iter().sum::<f64>() / letters.len() as f64
    }
}

/// The width of a character of a monospaced East Asian face, in thousandths of an em: an em for
/// ideographs and the symbols of Japanese and Chinese, and half an em for the rest.
fn monospaced_width(character: char) -> f64 {
    let code = character as u32;
    if takes_no_room(character) {
        return 0.0;
    }
    if is_wide(code) || FULL_WIDTH_SYMBOLS.contains(&character) || (8592..=9983).contains(&code) {
        THOUSANDTHS
    } else {
        500.0
    }
}

/// The symbols a monospaced East Asian face draws a full em wide.
const FULL_WIDTH_SYMBOLS: &[char] = &[
    '§', '¨', '°', '±', '´', '¶', '×', '÷', '‐', '―', '‖', '‘', '’', '“', '”', '†', '‡', '‥', '…',
    '‰', '′', '″', '※', '℃', 'Å',
];

/// Whether a character is one Word draws an em wide: an ideograph, a full-width form, a Hangul
/// syllable or a compatibility ideograph.
fn is_wide(code: u32) -> bool {
    (4352..=4447).contains(&code)
        || (11904..=42191).contains(&code)
        || (44032..=55203).contains(&code)
        || (63744..=64255).contains(&code)
        || (65072..=65103).contains(&code)
        || (65280..=65376).contains(&code)
        || (65504..=65510).contains(&code)
        || code >= 127_744
}

/// Whether a character is a half-width form of a CJK one.
fn is_half_width(code: u32) -> bool {
    (65377..=65500).contains(&code)
}

/// Whether a character takes no room of its own: a mark, an enclosing mark or a format character.
fn takes_no_room(character: char) -> bool {
    matches!(character as u32, 0x0300..=0x036F)
        || matches!(character, '\u{20D0}'..='\u{20FF}' | '\u{FE00}'..='\u{FE0F}')
        || matches!(character, '\u{200B}'..='\u{200F}' | '\u{2060}'..='\u{2064}')
}

// ── Arabic ───────────────────────────────────────────────────────────────────────────────────────

/// How a character joins the letters beside it: a dual-joining letter, a right-joining one, a
/// join-causing one, a transparent mark, or one of another script.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Joining {
    Dual,
    Right,
    Cause,
    Transparent,
    Other,
}

fn joining_of(character: char) -> Joining {
    match character {
        '\u{0640}' | '\u{200D}' => Joining::Cause,
        '\u{200C}' | '\u{0621}' => Joining::Other,
        other if takes_no_room(other) => Joining::Transparent,
        other if "آأؤإاةدذرزوژ".contains(other) => Joining::Right,
        other if is_joined_letter(other) => Joining::Dual,
        _ => Joining::Other,
    }
}

/// Whether a character is one of Arabic's letters the widths have, or the tatweel.
fn is_joined_letter(character: char) -> bool {
    character == '\u{0640}' || WIDTHS.letters.contains(character)
}

/// The widths of the letters of text Word joins, by index, in thousandths of an em.
#[expect(
    clippy::cast_possible_wrap,
    clippy::cast_sign_loss,
    reason = "an index into the text, stepped past the marks that join to the letter before it"
)]
fn joined_widths(characters: &[char], arabic: &[Option<f64>]) -> HashMap<usize, f64> {
    let joinings: Vec<Joining> = characters.iter().map(|c| joining_of(*c)).collect();
    let next_letter = |index: usize, step: isize| -> usize {
        let mut at = index as isize + step;
        while at >= 0
            && (at as usize) < joinings.len()
            && joinings[at as usize] == Joining::Transparent
        {
            at += step;
        }
        if at < 0 || at as usize >= joinings.len() {
            usize::MAX
        } else {
            at as usize
        }
    };
    let width = |at: usize| arabic.get(at).copied().flatten().unwrap_or_default();
    let mut widths = HashMap::new();
    let ligatures = WIDTHS.letters.chars().count() * 4;
    for (index, character) in characters.iter().enumerate() {
        if !is_joined_letter(*character) {
            continue;
        }
        if *character == '\u{0640}' {
            widths.insert(
                index,
                width(ligatures + WIDTHS.lam_alefs.chars().count() * 2),
            );
            continue;
        }
        let joining = joinings[index];
        let before = next_letter(index, -1);
        let after = next_letter(index, 1);
        let joins_before = matches!(joining, Joining::Dual | Joining::Right)
            && before != usize::MAX
            && matches!(joinings[before], Joining::Dual | Joining::Cause);
        let next = characters.get(after).copied();
        let alef = next
            .and_then(|next| WIDTHS.lam_alefs.chars().position(|c| c == next))
            .unwrap_or(usize::MAX);
        if *character == '\u{0644}' && alef != usize::MAX {
            widths.insert(
                index,
                width(ligatures + alef * 2 + usize::from(joins_before)),
            );
            widths.insert(after, 0.0);
            continue;
        }
        if widths.contains_key(&index) {
            continue;
        }
        let joins_after = joining == Joining::Dual
            && after != usize::MAX
            && matches!(
                joinings[after],
                Joining::Dual | Joining::Right | Joining::Cause
            );
        let form = match (joins_before, joins_after) {
            (true, true) => 3,
            (true, false) => 1,
            (false, true) => 2,
            (false, false) => 0,
        };
        let letter = WIDTHS
            .letters
            .chars()
            .position(|c| c == *character)
            .unwrap_or_default();
        widths.insert(index, width(letter * 4 + form));
    }
    widths
}

// ── the measurer ─────────────────────────────────────────────────────────────────────────────────

/// How wide `text` is in `font`, in points. A tab moves to the next whole tab stop, counted from the
/// start of the line.
pub(crate) fn measure_text_width(text: &str, font: &Font, start: f64) -> f64 {
    let face = WIDTHS.decoded(font);
    let more = WIDTHS.more_of(font);
    let arabic = WIDTHS.arabic_of(font);
    let size = font.size();
    let scale = font.scale_fraction();
    let characters: Vec<char> = text.chars().collect();
    let joined = arabic.and_then(|arabic| {
        characters
            .iter()
            .any(|character| is_joined_letter(*character))
            .then(|| joined_widths(&characters, arabic))
    });
    let mut position = start;
    for (index, character) in characters.iter().enumerate() {
        if *character == '\t' {
            let stop = tab_stop();
            // A plain multiply and add, as the reference writes it: a fused operation would round
            // once where the reference rounds twice.
            position = (position / stop).floor() * stop + stop;
            continue;
        }
        let width = if font.monospaced {
            monospaced_width(*character)
        } else {
            joined
                .as_ref()
                .and_then(|joined| joined.get(&index).copied())
                .unwrap_or_else(|| character_width(face, more, *character))
        };
        position += width * size * scale / 100_000.0 + font.character_spacing;
    }
    position - start
}

/// How tall a line of `font` is, in points.
pub(crate) fn measure_line_height(font: &Font) -> f64 {
    if let Some(face) = east_asian_face(font.face()) {
        return face.line_height * font.size() / THOUSANDTHS;
    }
    WIDTHS.face_of(font).line_height * font.size() / THOUSANDTHS
}

/// How far a line of `font` goes below its baseline, in points.
pub(crate) fn measure_descent(font: &Font) -> f64 {
    if let Some(face) = east_asian_face(font.face()) {
        return face.descent * font.size() / THOUSANDTHS;
    }
    WIDTHS.face_of(font).descent * font.size() / THOUSANDTHS
}

/// The first character of `text` whose width in its font is not known, so is not what Word lays out.
pub(crate) fn unknown_character(text: &str, font: &Font) -> Option<char> {
    if font.monospaced {
        // A monospaced East Asian face draws every character it is given, so only a private-use
        // character is one Word draws elsewhere.
        return text.chars().find(|character| {
            let code = *character as u32;
            (0xE000..=0xF8FF).contains(&code)
        });
    }
    let face = WIDTHS.decoded(font);
    let more = WIDTHS.more_of(font);
    let joined = WIDTHS.arabic_of(font).is_some() && font.character_spacing == 0.0;
    text.chars().find(|character| {
        let code = *character as u32;
        let index = WIDTHS.index.get(&code);
        let more_index = WIDTHS.more_index.get(&code);
        if is_joined_letter(*character) {
            return !joined;
        }
        if index.is_none() && more_index.is_none() {
            return false;
        }
        match index {
            None => more
                .and_then(|more| more.widths.get(more_index.copied().unwrap_or_default()))
                .copied()
                .flatten()
                .is_none(),
            Some(at) => face.widths.get(*at).copied().flatten().is_none(),
        }
    })
}

/// Whether the tables measure text in this font as another font, because they do not hold its own
/// widths. The layout stops there rather than guessing, as the reference does.
pub(crate) fn unknown_font(font: &Font) -> bool {
    let name = font.face();
    if is_east_asian_font(name) {
        return false;
    }
    let exact = alias(&WIDTHS.same_width_faces, name).is_some()
        || WIDTHS
            .faces
            .iter()
            .any(|face| face.name.eq_ignore_ascii_case(name));
    !exact
}

/// The line height of a run of text in `font`, where characters Word draws in another face make the
/// line taller. Undefined when the whole of it is drawn in the font's own face.
pub(crate) fn measure_text_height(text: &str, font: &Font) -> Option<f64> {
    if font.monospaced {
        // A monospaced East Asian face's line is its own, whatever the text.
        return None;
    }
    let face = WIDTHS.decoded(font);
    let more = WIDTHS.more_of(font);
    let size = font.size();
    let own = (
        measure_line_height(font) - measure_descent(font),
        measure_descent(font),
    );
    let mut tallest = own;
    let mut other = false;
    for character in text.chars() {
        if character.is_whitespace() || takes_no_room(character) {
            continue;
        }
        let code = character as u32;
        let height = WIDTHS
            .index
            .get(&code)
            .and_then(|at| face.fallbacks.get(*at).copied().flatten())
            .map(|fallback| {
                let font = &WIDTHS.fallback_faces[fallback];
                let other = WIDTHS
                    .faces
                    .iter()
                    .find(|face| face.name.eq_ignore_ascii_case(&font.font))
                    .unwrap_or(&WIDTHS.faces[0]);
                (
                    (other.line_height - other.descent) * size / THOUSANDTHS,
                    other.descent * size / THOUSANDTHS,
                )
            })
            .or_else(|| {
                WIDTHS
                    .more_index
                    .get(&code)
                    .and_then(|at| more?.fonts.get(*at).copied().flatten())
                    .and_then(|at| WIDTHS.fallback_fonts.get(at))
                    .map(|font| {
                        (
                            font.ascent * size / THOUSANDTHS,
                            font.descent * size / THOUSANDTHS,
                        )
                    })
            })
            .unwrap_or(own);
        if height.0 > tallest.0 || height.1 > tallest.1 {
            other = true;
            tallest = (tallest.0.max(height.0), tallest.1.max(height.1));
        }
    }
    other.then_some(tallest.0 + tallest.1)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_default_face_measures_a_created_documents_text() {
        // The reference's own measurement of a created document's text: Times New Roman at ten
        // points, because neither the run nor the style chain names a font.
        let font = Font::default();
        assert!((measure_text_width("Hello world", &font, 0.0) - 48.05).abs() < 1e-9);
        assert!((measure_line_height(&font) - 11.499_023_437_5).abs() < 1e-9);
    }

    #[test]
    fn a_joined_arabic_run_is_narrower_than_its_letters_alone() {
        // The property the Arabic case's first check reads: Word joins the letters, and the joined
        // forms are narrower than the sum of the isolated advances. The face is the one a created
        // document's runs are measured in — Times New Roman, because a created run names no font —
        // and it has an Arabic table, so the join is what the measurer sees.
        let font = Font::default();
        let joined = measure_text_width("بب", &font, 0.0);
        let separate = measure_text_width("ب", &font, 0.0) * 2.0;
        assert!(
            joined < separate,
            "{joined} is not narrower than {separate}"
        );
    }

    #[test]
    fn a_tab_moves_to_the_next_whole_stop() {
        // The tab stop the layout data states, not a literal: 36 points by default.
        let font = Font::default();
        let from_zero = measure_text_width("\t", &font, 0.0);
        assert!((from_zero - tab_stop()).abs() < 1e-9, "{from_zero}");
        let from_ten = measure_text_width("\t", &font, 10.0);
        assert!((from_ten - (tab_stop() - 10.0)).abs() < 1e-9, "{from_ten}");
    }

    #[test]
    fn a_character_the_face_does_not_state_is_reported() {
        // U+2207 (nabla) is outside every table, so the layout stops there rather than guessing.
        assert_eq!(
            unknown_character("\u{2207}", &Font::default()),
            Some('\u{2207}')
        );
        assert_eq!(unknown_character("Hello", &Font::default()), None);
    }
}

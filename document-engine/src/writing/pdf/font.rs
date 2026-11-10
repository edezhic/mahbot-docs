//! The one face a produced PDF draws with, and the subset a call embeds.
//!
//! The bundled font is parsed once, by hand, out of the trusted asset: the sfnt directory, `head`,
//! `hhea`, `maxp`, `hmtx`, `loca`, `glyf` and the character map, plus the three hinting tables the
//! subset copies verbatim. What a produced file embeds is not that font but the glyphs it draws: a
//! subset that keeps exactly the tables the reference's library keeps — `head`, `hhea`, `loca`,
//! `maxp`, `cvt `, `prep`, `glyf`, `hmtx` and `fpgm`, and no `cmap`, `post`, `name` or `OS/2` — with
//! glyph 0 first and every requested glyph renumbered consecutively in first-use order, so the CID
//! a character draws with is the glyph's index in the subset. A composite glyph names its
//! components by glyph id, so the components join the subset behind the requested glyphs and each
//! record's id is rewritten to the component's CID; without that rewrite the subset would point at
//! glyphs it does not carry. The file's `head.checksumAdjustment` is what makes the whole program
//! sum to `0xB1B0AFBA`, so it is written last, over the file the tables make.
//!
//! The parser is written here rather than delegated to a font crate. `skrifa`/`read-fonts` are
//! already resolved through hayro and already recorded in the comparison's codec list, so naming
//! one would change neither the lockfile nor the recorded identity — but it would tie this
//! manifest's exact pin to whatever version hayro resolves, and what it would replace is the
//! reading of one trusted asset the crate ships: the cmap, the composite glyphs and the metrics.
//! The subset's own work — renumbering glyphs, assembling the nine-table sfnt and its checksum, and
//! the PDF-side font dictionary — is hand-written either way.

use std::collections::HashMap;
use std::sync::LazyLock;

/// The bytes of one sfnt table record: tag, checksum, offset and length.
const RECORD: usize = 16;
/// Tables and glyphs are aligned to this many bytes.
const ALIGN: usize = 4;
/// One `hmtx` entry, and one 32-bit word.
const WORD: usize = 4;
/// The first table's offset: the offset table and one record per table.
const DIRECTORY: usize = 12;
/// The bytes of `head` a subset copies.
const HEAD: usize = 54;
/// The bytes of `hhea` a subset copies.
const HHEA: usize = 36;
/// The bytes of `maxp` a subset copies.
const MAXP: usize = 32;
/// A composite glyph's header, before its first component record.
const COMPOSITE_HEADER: usize = 10;
/// `ARG_1_AND_2_ARE_WORDS`: the component's arguments are two `i16`s rather than two `i8`s.
const ARGUMENTS_ARE_WORDS: u16 = 0x0001;
/// `WE_HAVE_A_SCALE`: one `F2Dot14` scale follows the arguments.
const HAVE_A_SCALE: u16 = 0x0008;
/// `MORE_COMPONENTS`: another component record follows this one.
const MORE_COMPONENTS: u16 = 0x0020;
/// `WE_HAVE_AN_X_AND_Y_SCALE`: two `F2Dot14` scales follow the arguments.
const HAVE_X_AND_Y_SCALE: u16 = 0x0040;
/// `WE_HAVE_A_TWO_BY_TWO`: four `F2Dot14` values follow the arguments.
const HAVE_TWO_BY_TWO: u16 = 0x0080;

/// The bundled PDF face, parsed once.
pub(super) struct Face {
    /// `head`, as copied into a subset.
    head: &'static [u8],
    /// `hhea`, as copied into a subset.
    hhea: &'static [u8],
    /// `maxp`, as copied into a subset.
    maxp: &'static [u8],
    /// Font units to the em, which is also the glyph space a PDF states widths in.
    units_per_em: u16,
    /// `hhea`'s ascent and descent, in font units.
    ascender: i16,
    descender: i16,
    /// `head`'s bounding box, in font units.
    bbox: [i16; 4],
    /// `OS/2`'s cap height and x height, or the ascent when the table states none.
    cap_height: i16,
    x_height: i16,
    /// How many glyphs `loca` and `hmtx` describe.
    num_glyphs: u16,
    /// Every glyph's `hmtx` entry.
    metrics: Vec<Metric>,
    /// Every glyph's start in `glyf`, one more than the glyph count of them.
    locations: Vec<u32>,
    glyf: &'static [u8],
    /// The hinting tables a subset carries verbatim.
    cvt: &'static [u8],
    prep: &'static [u8],
    fpgm: &'static [u8],
    /// The character map the face looks a character up in.
    cmap: Mapping,
}

/// One glyph's advance and left side bearing, in font units.
#[derive(Clone, Copy)]
struct Metric {
    advance: u16,
    lsb: i16,
}

impl Face {
    /// The bundled font, parsed once. Panicking on the embedded asset is an engine defect, which
    /// the operation boundary contains; the asset is a build-time constant and its parse is tested.
    #[must_use]
    pub(super) fn bundled() -> &'static Face {
        static FACE: LazyLock<Face> = LazyLock::new(|| {
            Face::parse(crate::vocabulary::FONT)
                .unwrap_or_else(|reason| panic!("the bundled font: {reason}"))
        });
        &FACE
    }

    /// The face `font` states, or what about it cannot be read.
    fn parse(font: &'static [u8]) -> Result<Self, String> {
        let head = table(font, *b"head")?;
        let hhea = table(font, *b"hhea")?;
        let maxp = table(font, *b"maxp")?;
        let hmtx = table(font, *b"hmtx")?;
        let loca = table(font, *b"loca")?;
        let glyf = table(font, *b"glyf")?;
        let cmap = table(font, *b"cmap")?;
        let cvt = table(font, *b"cvt ")?;
        let prep = table(font, *b"prep")?;
        let fpgm = table(font, *b"fpgm")?;

        let units_per_em = u16_at(head, 18).ok_or("`head` states no units per em")?;
        let long_loca = i16_at(head, 50).ok_or("`head` states no loca format")? == 1;
        let ascender = i16_at(hhea, 4).ok_or("`hhea` states no ascent")?;
        let descender = i16_at(hhea, 6).ok_or("`hhea` states no descent")?;
        let number_of_h_metrics = u16_at(hhea, 34).ok_or("`hhea` states no metric count")?;
        let num_glyphs = u16_at(maxp, 4).ok_or("`maxp` states no glyph count")?;
        let bbox = [
            i16_at(head, 36).ok_or("`head` states no bounding box")?,
            i16_at(head, 38).ok_or("`head` states no bounding box")?,
            i16_at(head, 40).ok_or("`head` states no bounding box")?,
            i16_at(head, 42).ok_or("`head` states no bounding box")?,
        ];
        let os2 = table(font, *b"OS/2").ok();
        let cap_height = os2.and_then(|table| i16_at(table, 88)).unwrap_or(ascender);
        let x_height = os2
            .and_then(|table| i16_at(table, 86))
            .unwrap_or(ascender / 2);

        Ok(Self {
            head: head.get(..HEAD).ok_or("`head` is short")?,
            hhea: hhea.get(..HHEA).ok_or("`hhea` is short")?,
            maxp: maxp.get(..MAXP).ok_or("`maxp` is short")?,
            units_per_em,
            ascender,
            descender,
            bbox,
            cap_height,
            x_height,
            num_glyphs,
            metrics: metrics(hmtx, num_glyphs, number_of_h_metrics),
            locations: locations(loca, num_glyphs, long_loca),
            glyf,
            cvt,
            prep,
            fpgm,
            cmap: Mapping::parse(cmap)?,
        })
    }

    /// The glyph a character maps to, or 0 when the font has no glyph for it.
    #[must_use]
    pub(super) fn glyph(&self, character: char) -> u16 {
        let glyph = self.cmap.lookup(u32::from(character));
        if glyph < self.num_glyphs { glyph } else { 0 }
    }

    /// The advance width of a glyph, in font units.
    #[must_use]
    pub(super) fn advance(&self, glyph: u16) -> u16 {
        self.metric(glyph).advance
    }

    #[must_use]
    pub(super) fn units_per_em(&self) -> u16 {
        self.units_per_em
    }

    #[must_use]
    pub(super) fn ascender(&self) -> i16 {
        self.ascender
    }

    #[must_use]
    pub(super) fn descender(&self) -> i16 {
        self.descender
    }

    #[must_use]
    pub(super) fn font_bbox(&self) -> [i16; 4] {
        self.bbox
    }

    #[must_use]
    pub(super) fn cap_height(&self) -> i16 {
        self.cap_height
    }

    #[must_use]
    pub(super) fn x_height(&self) -> i16 {
        self.x_height
    }

    /// `sum(advance(glyph(c)) for c in text) * size / units_per_em`. No kerning: pdf-lib's
    /// widthOfTextAtSize applies none for this font (verified against the reference).
    #[must_use]
    pub(super) fn width(&self, text: &str, size: f64) -> f64 {
        let units = text.chars().map(|character| self.units(character)).sum();
        self.scaled(units, size)
    }

    /// The font units one character advances, which is what [`Face::width`] sums.
    #[must_use]
    pub(super) fn units(&self, character: char) -> u64 {
        u64::from(self.advance(self.glyph(character)))
    }

    /// The width `units` font units take at `size`, exactly as [`Face::width`] scales a sum of them.
    #[must_use]
    pub(super) fn scaled(&self, units: u64, size: f64) -> f64 {
        let units = f64::from(u32::try_from(units).unwrap_or(u32::MAX));
        units * size / f64::from(self.units_per_em())
    }

    /// The face's ascent at `size`: the part of the em above the baseline, which a single-line
    /// appearance centres.
    #[must_use]
    pub(super) fn ascent(&self, size: f64) -> f64 {
        f64::from(self.ascender) * size / f64::from(self.units_per_em())
    }

    /// One line's height at `size`: `(ascender - descender) * size / units_per_em`.
    #[must_use]
    pub(super) fn height(&self, size: f64) -> f64 {
        let em = f64::from(self.units_per_em());
        f64::from(self.ascender) * size / em - f64::from(self.descender) * size / em
    }

    /// The subset carrying every non-zero glyph `characters` name, in first-use order.
    #[must_use]
    pub(super) fn subset(&self, characters: &[char]) -> Subset {
        let mut remap = Remapper::new();
        let mut codes = HashMap::new();
        // The first character each CID was requested for, in CID order: glyph 0 has none, and
        // neither does a glyph only a composite pulled in.
        let mut first_use: Vec<Option<char>> = vec![None];
        for &character in characters {
            let glyph = self.glyph(character);
            if glyph == 0 {
                continue;
            }
            let cid = remap.add(glyph);
            codes.insert(character, cid);
            while first_use.len() <= usize::from(cid) {
                first_use.push(None);
            }
            first_use[usize::from(cid)].get_or_insert(character);
        }

        // A composite names its components by glyph id, so each component joins the subset behind
        // the requested glyphs; the walk continues because a component may be composite itself.
        let mut next = 1;
        while next < remap.order.len() {
            let data = self.glyph_data(remap.order[next]);
            if is_composite(data) {
                for component in components(data) {
                    remap.add(component.glyph);
                }
            }
            next += 1;
        }

        let count =
            u16::try_from(remap.order.len()).expect("a TrueType font holds at most 65535 glyphs");
        let mut glyf = Vec::new();
        let mut locations = Vec::with_capacity(remap.order.len() + 1);
        locations.push(0);
        let mut metrics = Vec::with_capacity(remap.order.len());
        for &glyph in &remap.order {
            let data = self.glyph_data(glyph);
            if is_composite(data) {
                let mut rewritten = data.to_vec();
                for component in components(data) {
                    let cid = remap.map.get(&component.glyph).copied().unwrap_or(0);
                    rewritten[component.at..component.at + 2].copy_from_slice(&cid.to_be_bytes());
                }
                glyf.extend_from_slice(&rewritten);
            } else {
                glyf.extend_from_slice(data);
            }
            glyf.resize(glyf.len().next_multiple_of(ALIGN), 0);
            locations.push(u32::try_from(glyf.len()).unwrap_or(u32::MAX));
            metrics.push(self.metric(glyph));
        }

        let mut head = self.head.to_vec();
        // The adjustment is written over the assembled program, once every table's checksum is
        // known; `indexToLocFormat` states the long offsets this subset writes.
        head[8..12].fill(0);
        head[50..52].copy_from_slice(&1_i16.to_be_bytes());

        let mut hhea = self.hhea.to_vec();
        hhea[34..36].copy_from_slice(&count.to_be_bytes());

        let mut maxp = self.maxp.to_vec();
        maxp[4..6].copy_from_slice(&count.to_be_bytes());

        let mut hmtx = Vec::with_capacity(metrics.len() * WORD);
        for metric in &metrics {
            hmtx.extend_from_slice(&metric.advance.to_be_bytes());
            hmtx.extend_from_slice(&metric.lsb.to_be_bytes());
        }

        let mut loca = Vec::with_capacity(locations.len() * WORD);
        for offset in &locations {
            loca.extend_from_slice(&offset.to_be_bytes());
        }

        let program = assemble(vec![
            (b"cvt ", self.cvt.to_vec()),
            (b"fpgm", self.fpgm.to_vec()),
            (b"glyf", glyf),
            (b"head", head),
            (b"hhea", hhea),
            (b"hmtx", hmtx),
            (b"loca", loca),
            (b"maxp", maxp),
            (b"prep", self.prep.to_vec()),
        ]);

        Subset {
            program,
            codes,
            widths: remap
                .order
                .iter()
                .map(|&glyph| glyph_space(self.advance(glyph), self.units_per_em))
                .collect(),
            unicode: first_use
                .iter()
                .enumerate()
                .skip(1)
                .filter_map(|(cid, &character)| {
                    character.map(|character| {
                        (u16::try_from(cid).expect("a glyph id is a u16"), character)
                    })
                })
                .collect(),
        }
    }

    /// A glyph's `hmtx` entry, with glyph 0 for a glyph the face does not describe.
    fn metric(&self, glyph: u16) -> Metric {
        self.metrics
            .get(usize::from(glyph))
            .copied()
            .unwrap_or(Metric { advance: 0, lsb: 0 })
    }

    /// The glyph's bytes in `glyf`, empty when `loca` gives it none.
    fn glyph_data(&self, glyph: u16) -> &'static [u8] {
        let index = usize::from(glyph);
        let (Some(&start), Some(&end)) = (self.locations.get(index), self.locations.get(index + 1))
        else {
            return &[];
        };
        let start = usize::try_from(start).unwrap_or(usize::MAX);
        let end = usize::try_from(end).unwrap_or(0);
        self.glyf.get(start..end).unwrap_or(&[])
    }
}

/// One subset font and the CID mapping its caller draws with.
pub(super) struct Subset {
    /// The FontFile2 program.
    pub(super) program: Vec<u8>,
    /// The CID for a character: 0 when the font cannot draw it, otherwise the remapped glyph id.
    pub(super) codes: HashMap<char, u16>,
    /// Advance widths indexed by CID, in 1000-unit glyph space (the PDF `/W` values).
    pub(super) widths: Vec<u16>,
    /// The character each non-zero CID draws, in CID order: the `/ToUnicode` map.
    pub(super) unicode: Vec<(u16, char)>,
}

/// A glyph's advance in the 1000-unit glyph space a PDF states widths in.
fn glyph_space(advance: u16, units_per_em: u16) -> u16 {
    let scaled = u32::from(advance) * 1000 + u32::from(units_per_em) / 2;
    u16::try_from(scaled / u32::from(units_per_em)).unwrap_or(u16::MAX)
}

/// The glyph ids a subset keeps, in CID order.
struct Remapper {
    /// `order[cid]` is the original glyph id the CID stands for.
    order: Vec<u16>,
    /// The CID an original glyph id was given.
    map: HashMap<u16, u16>,
}

impl Remapper {
    /// The remapper of every subset: glyph 0, the missing-glyph, is CID 0.
    fn new() -> Self {
        let mut map = HashMap::new();
        map.insert(0, 0);
        Self {
            order: vec![0],
            map,
        }
    }

    /// The CID for a glyph, giving it the next one when it is new.
    fn add(&mut self, glyph: u16) -> u16 {
        if let Some(&cid) = self.map.get(&glyph) {
            return cid;
        }
        let cid = u16::try_from(self.order.len()).expect("a subset never outgrows a u16 CID");
        self.order.push(glyph);
        self.map.insert(glyph, cid);
        cid
    }
}

/// One component a composite glyph names, and where its glyph id sits in the glyph's bytes.
#[derive(Clone, Copy)]
struct Component {
    glyph: u16,
    at: usize,
}

/// Whether a glyph's data is a composite rather than an outline.
fn is_composite(data: &[u8]) -> bool {
    i16_at(data, 0).is_some_and(|contours| contours < 0)
}

/// The component records a composite glyph names, in the order it names them.
fn components(data: &[u8]) -> Vec<Component> {
    let mut components = Vec::new();
    let mut at = COMPOSITE_HEADER;
    while let (Some(flags), Some(glyph)) = (u16_at(data, at), u16_at(data, at + 2)) {
        components.push(Component { glyph, at: at + 2 });
        at += 4;
        at += if flags & ARGUMENTS_ARE_WORDS == 0 {
            2
        } else {
            4
        };
        at += if flags & HAVE_A_SCALE != 0 {
            2
        } else if flags & HAVE_X_AND_Y_SCALE != 0 {
            4
        } else if flags & HAVE_TWO_BY_TWO != 0 {
            8
        } else {
            0
        };
        if flags & MORE_COMPONENTS == 0 {
            break;
        }
    }
    components
}

/// Every glyph's `hmtx` entry. Past `number_of_h_metrics` the advance repeats the last one and only
/// the left side bearing is stated, one after another.
fn metrics(hmtx: &[u8], num_glyphs: u16, number_of_h_metrics: u16) -> Vec<Metric> {
    let stated = usize::from(number_of_h_metrics);
    let last = stated.saturating_sub(1);
    let mut metrics = Vec::with_capacity(usize::from(num_glyphs));
    for glyph in 0..num_glyphs {
        let index = usize::from(glyph);
        let (advance, lsb) = if index < stated {
            let at = index * WORD;
            (
                u16_at(hmtx, at).unwrap_or(0),
                i16_at(hmtx, at + 2).unwrap_or(0),
            )
        } else {
            let at = stated * WORD + (index - stated) * 2;
            (
                u16_at(hmtx, last * WORD).unwrap_or(0),
                i16_at(hmtx, at).unwrap_or(0),
            )
        };
        metrics.push(Metric { advance, lsb });
    }
    metrics
}

/// Every glyph's start in `glyf`, short offsets doubled and long ones taken as they are.
fn locations(loca: &[u8], num_glyphs: u16, long: bool) -> Vec<u32> {
    let mut locations = Vec::with_capacity(usize::from(num_glyphs) + 1);
    for index in 0..=usize::from(num_glyphs) {
        let at = if long { index * WORD } else { index * 2 };
        let offset = if long {
            u32_at(loca, at).unwrap_or(0)
        } else {
            u32::from(u16_at(loca, at).unwrap_or(0)) * 2
        };
        locations.push(offset);
    }
    locations
}

/// A table's bytes, or what the directory says about it that cannot be read.
fn table(font: &[u8], tag: [u8; 4]) -> Result<&[u8], String> {
    let count = u16_at(font, 4).ok_or("the font states no table count")?;
    for index in 0..count {
        let at = DIRECTORY + RECORD * usize::from(index);
        let Some(record) = font.get(at..at + RECORD) else {
            break;
        };
        if record[..4] != tag {
            continue;
        }
        let offset = usize::try_from(u32_at(record, 8).unwrap_or(0)).unwrap_or(usize::MAX);
        let length = usize::try_from(u32_at(record, 12).unwrap_or(0)).unwrap_or(0);
        return font
            .get(offset..offset.saturating_add(length))
            .ok_or_else(|| format!("the `{}` table is out of bounds", name(tag)));
    }
    Err(format!("the font states no `{}` table", name(tag)))
}

/// A table tag as text, for a message.
fn name(tag: [u8; 4]) -> String {
    String::from_utf8_lossy(&tag).into_owned()
}

/// The sfnt program the nine tables make: the directory, the tables, and `head`'s adjustment.
fn assemble(mut tables: Vec<(&'static [u8; 4], Vec<u8>)>) -> Vec<u8> {
    tables.sort_by(|left, right| left.0.cmp(right.0));
    let count = u16::try_from(tables.len()).expect("a subset holds nine tables");
    let record = u16::try_from(RECORD).expect("an sfnt record is sixteen bytes");
    // The offset table's search parameters, as the format states them: the largest power of two not
    // above the table count, and the bytes left over after that many records. An empty list — a
    // caller defect, never a subset — states zeroes rather than underflowing.
    let (search_range, entry_selector, range_shift) = match count.checked_ilog2() {
        Some(log) => {
            let power = 1_u16 << log;
            let search = power * record;
            (
                search,
                u16::try_from(log).expect("a log2 of a table count fits a u16"),
                count * record - search,
            )
        }
        None => (0, 0, 0),
    };

    let mut offsets = Vec::with_capacity(tables.len());
    let mut at = DIRECTORY + RECORD * tables.len();
    for (_, data) in &tables {
        offsets.push(at);
        at = (at + data.len()).next_multiple_of(ALIGN);
    }

    let mut program = Vec::with_capacity(at);
    program.extend_from_slice(&[0x00, 0x01, 0x00, 0x00]);
    program.extend_from_slice(&count.to_be_bytes());
    program.extend_from_slice(&search_range.to_be_bytes());
    program.extend_from_slice(&entry_selector.to_be_bytes());
    program.extend_from_slice(&range_shift.to_be_bytes());
    for ((tag, data), &offset) in tables.iter().zip(&offsets) {
        program.extend_from_slice(*tag);
        program.extend_from_slice(&checksum(data).to_be_bytes());
        program.extend_from_slice(&u32::try_from(offset).unwrap_or(0).to_be_bytes());
        program.extend_from_slice(&u32::try_from(data.len()).unwrap_or(0).to_be_bytes());
    }
    for ((_, data), &offset) in tables.iter().zip(&offsets) {
        program.resize(offset, 0);
        program.extend_from_slice(data);
    }
    program.resize(at, 0);

    // `head.checksumAdjustment` is what makes the whole program sum to the magic constant: the sum
    // of every word, with the field still zero, subtracted from it.
    if let Some(index) = tables.iter().position(|(tag, _)| *tag == b"head") {
        let adjustment = 0xB1B0_AFBA_u32.wrapping_sub(checksum(&program));
        let at = offsets[index] + 8;
        if let Some(field) = program.get_mut(at..at + WORD) {
            field.copy_from_slice(&adjustment.to_be_bytes());
        }
    }
    program
}

/// A table's checksum: its bytes as big-endian words, summed, the last word zero-padded.
fn checksum(data: &[u8]) -> u32 {
    let mut sum = 0_u32;
    for word in data.chunks(WORD) {
        let mut bytes = [0_u8; WORD];
        bytes[..word.len()].copy_from_slice(word);
        sum = sum.wrapping_add(u32::from_be_bytes(bytes));
    }
    sum
}

/// A big-endian `u16` at `at`.
fn u16_at(data: &[u8], at: usize) -> Option<u16> {
    let bytes: [u8; 2] = data.get(at..at + 2)?.try_into().ok()?;
    Some(u16::from_be_bytes(bytes))
}

/// A big-endian `i16` at `at`.
fn i16_at(data: &[u8], at: usize) -> Option<i16> {
    u16_at(data, at).map(u16::cast_signed)
}

/// A big-endian `u32` at `at`.
fn u32_at(data: &[u8], at: usize) -> Option<u32> {
    let bytes: [u8; WORD] = data.get(at..at + WORD)?.try_into().ok()?;
    Some(u32::from_be_bytes(bytes))
}

/// The character map a face looks a character up in.
enum Mapping {
    /// Format 4: the segmented map of the basic multilingual plane.
    Segmented {
        starts: Vec<u16>,
        ends: Vec<u16>,
        deltas: Vec<i16>,
        offsets: Vec<u16>,
        /// The subtable's own bytes, which `offsets` index into for a glyph id array.
        data: &'static [u8],
        /// Where the range-offset array starts in `data`.
        range_offsets_at: usize,
    },
    /// Format 12: the group map that reaches past the plane.
    Grouped(Vec<(u32, u32, u32)>),
}

impl Mapping {
    /// The character map `cmap` states, preferring the BMP's format 4 and falling back to format 12
    /// and then to any Unicode subtable.
    fn parse(cmap: &'static [u8]) -> Result<Self, String> {
        let count = u16_at(cmap, 2).ok_or("`cmap` states no subtable count")?;
        let mut candidates = Vec::new();
        for index in 0..count {
            let at = 4 + 8 * usize::from(index);
            let Some(record) = cmap.get(at..at + 8) else {
                break;
            };
            let platform = u16_at(record, 0).unwrap_or(0);
            let encoding = u16_at(record, 2).unwrap_or(0);
            if !(platform == 0 || (platform == 3 && (encoding == 1 || encoding == 10))) {
                continue;
            }
            let Some(subtable) = usize::try_from(u32_at(record, 4).unwrap_or(0))
                .ok()
                .and_then(|offset| cmap.get(offset..))
            else {
                continue;
            };
            candidates.push((u16_at(subtable, 0).unwrap_or(0), subtable));
        }
        let chosen = candidates
            .iter()
            .find(|(format, _)| *format == 4)
            .or_else(|| candidates.iter().find(|(format, _)| *format == 12))
            .or_else(|| candidates.first())
            .ok_or("`cmap` states no Unicode subtable")?;
        Self::from_subtable(chosen.0, chosen.1)
    }

    /// The mapping a subtable of `format` states.
    fn from_subtable(format: u16, data: &'static [u8]) -> Result<Self, String> {
        match format {
            4 => {
                let span = usize::from(
                    u16_at(data, 6).ok_or("the character map states no segment count")?,
                );
                let count = span / 2;
                let ends_at = 14;
                let starts_at = ends_at + span + 2;
                let deltas_at = starts_at + span;
                let range_offsets_at = deltas_at + span;
                let mut starts = Vec::with_capacity(count);
                let mut ends = Vec::with_capacity(count);
                let mut deltas = Vec::with_capacity(count);
                let mut offsets = Vec::with_capacity(count);
                for index in 0..count {
                    let at = 2 * index;
                    starts.push(
                        u16_at(data, starts_at + at).ok_or("the character map is truncated")?,
                    );
                    ends.push(u16_at(data, ends_at + at).ok_or("the character map is truncated")?);
                    deltas.push(
                        i16_at(data, deltas_at + at).ok_or("the character map is truncated")?,
                    );
                    offsets.push(
                        u16_at(data, range_offsets_at + at)
                            .ok_or("the character map is truncated")?,
                    );
                }
                Ok(Self::Segmented {
                    starts,
                    ends,
                    deltas,
                    offsets,
                    data,
                    range_offsets_at,
                })
            }
            12 => {
                let count = u32_at(data, 12).ok_or("the character map states no group count")?;
                let count = usize::try_from(count).unwrap_or(usize::MAX);
                if count.saturating_mul(12).saturating_add(16) > data.len() {
                    return Err("the character map is truncated".into());
                }
                let mut groups = Vec::with_capacity(count);
                for index in 0..count {
                    let at = 16 + 12 * index;
                    groups.push((
                        u32_at(data, at).ok_or("the character map is truncated")?,
                        u32_at(data, at + 4).ok_or("the character map is truncated")?,
                        u32_at(data, at + 8).ok_or("the character map is truncated")?,
                    ));
                }
                Ok(Self::Grouped(groups))
            }
            other => Err(format!(
                "the character map's format {other} is not one this face reads"
            )),
        }
    }

    /// The glyph a code maps to, or 0 when the map has none.
    fn lookup(&self, code: u32) -> u16 {
        match self {
            Self::Segmented {
                starts,
                ends,
                deltas,
                offsets,
                data,
                range_offsets_at,
            } => {
                // The segment starts are sorted, so the one code can fall in is found by halving
                // rather than scanning: the fitter asks for a glyph hundreds of times per value.
                let index = starts.partition_point(|&start| u32::from(start) <= code);
                if index == 0 {
                    return 0;
                }
                let index = index - 1;
                if code > u32::from(ends[index]) {
                    return 0;
                }
                let delta = deltas[index].cast_unsigned();
                let offset = offsets[index];
                if offset == 0 {
                    return u16::try_from(code).unwrap_or(0).wrapping_add(delta);
                }
                // The range offset is counted from its own place in the array, so the glyph id
                // is `offset` bytes on from the entry, plus two for each code past the start.
                let within = usize::try_from(code - u32::from(starts[index])).unwrap_or(0);
                let at = range_offsets_at + 2 * index + usize::from(offset) + 2 * within;
                match u16_at(data, at) {
                    Some(0) | None => 0,
                    Some(glyph) => glyph.wrapping_add(delta),
                }
            }
            Self::Grouped(groups) => {
                for &(start, end, glyph) in groups {
                    if start <= code && code <= end {
                        return u16::try_from(glyph + (code - start)).unwrap_or(0);
                    }
                }
                0
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Face, is_composite};
    use std::collections::{BTreeSet, HashMap};

    /// The sfnt tables a subset carries, read back out of the program it states.
    fn tables(program: &[u8]) -> Vec<(String, Vec<u8>)> {
        let count = usize::from(u16::from_be_bytes([program[4], program[5]]));
        let mut tables = Vec::new();
        for index in 0..count {
            let at = 12 + 16 * index;
            let tag = String::from_utf8_lossy(&program[at..at + 4]).into_owned();
            let offset = u32::from_be_bytes([
                program[at + 8],
                program[at + 9],
                program[at + 10],
                program[at + 11],
            ]);
            let length = u32::from_be_bytes([
                program[at + 12],
                program[at + 13],
                program[at + 14],
                program[at + 15],
            ]);
            let offset = usize::try_from(offset).expect("a table offset");
            let length = usize::try_from(length).expect("a table length");
            tables.push((tag, program[offset..offset + length].to_vec()));
        }
        tables
    }

    /// One table of a program, by tag.
    fn table_of(program: &[u8], tag: &str) -> Vec<u8> {
        tables(program)
            .into_iter()
            .find(|(found, _)| found == tag)
            .unwrap_or_else(|| panic!("the program states `{tag}`"))
            .1
    }

    /// A big-endian `u16` in a table.
    fn word(data: &[u8], at: usize) -> u16 {
        u16::from_be_bytes([data[at], data[at + 1]])
    }

    /// The checksum a table states about itself.
    fn checksum(data: &[u8]) -> u32 {
        let mut sum = 0_u32;
        for word in data.chunks(4) {
            let mut bytes = [0_u8; 4];
            bytes[..word.len()].copy_from_slice(word);
            sum = sum.wrapping_add(u32::from_be_bytes(bytes));
        }
        sum
    }

    #[test]
    fn the_bundled_font_parses_and_states_its_metrics() {
        let face = Face::bundled();
        assert_eq!(face.units_per_em(), 1000);
        assert_eq!(face.ascender(), 1069);
        assert_eq!(face.descender(), -293);
    }

    #[test]
    fn an_empty_table_list_states_zeroes_rather_than_underflowing() {
        let program = super::assemble(Vec::new());
        assert_eq!(program.len(), 12, "the offset table alone");
        assert_eq!(
            &program[4..12],
            &[0, 0, 0, 0, 0, 0, 0, 0],
            "no table count and no search parameters"
        );
    }

    #[test]
    fn glyph_lookup_and_width_match_the_reference() {
        let face = Face::bundled();
        assert_ne!(face.glyph('A'), 0);
        assert_eq!(face.glyph('漢'), 0);
        assert!((face.width("AVATAR", 24.0) - 88.68).abs() < 1e-9);
        assert!((face.height(24.0) - 32.688).abs() < 1e-9);
    }

    #[test]
    fn a_subset_carries_the_nine_tables_and_renumbers_its_glyphs() {
        let face = Face::bundled();
        let characters = ['A', 'V', 'A', 'T', 'A', 'R'];
        let subset = face.subset(&characters);

        let mut distinct = BTreeSet::new();
        for &character in &characters {
            distinct.insert(face.glyph(character));
        }
        let tags: BTreeSet<String> = tables(&subset.program)
            .into_iter()
            .map(|(tag, _)| tag)
            .collect();
        let expected: BTreeSet<String> = [
            "head", "hhea", "loca", "maxp", "cvt ", "prep", "glyf", "hmtx", "fpgm",
        ]
        .into_iter()
        .map(str::to_string)
        .collect();
        assert_eq!(tags, expected);
        let maxp = table_of(&subset.program, "maxp");
        assert_eq!(
            usize::from(word(&maxp, 4)),
            distinct.len() + 1,
            "glyph 0 and one CID per distinct glyph"
        );
        assert_eq!(
            subset.codes.len(),
            distinct.len(),
            "one CID per distinct glyph"
        );
        assert_ne!(subset.codes.get(&'A').copied().unwrap_or(0), 0);
        assert_eq!(
            subset
                .unicode
                .iter()
                .filter(|(_, character)| *character == 'A')
                .count(),
            1,
            "the repeated 'A' is one CID"
        );
        assert_eq!(subset.codes.get(&'漢').copied().unwrap_or(0), 0);
        assert!(subset.program.len() < 20_000);
    }

    #[test]
    fn a_composite_glyph_brings_its_components() {
        let face = Face::bundled();
        let subset = face.subset(&['Ё']);
        let maxp = table_of(&subset.program, "maxp");
        let count = usize::from(word(&maxp, 4));
        assert!(count > 2, "the composite and its components are carried");

        let glyf = table_of(&subset.program, "glyf");
        let loca = table_of(&subset.program, "loca");
        let mut composites = 0;
        for cid in 1..count {
            let start = usize::try_from(u32::from_be_bytes([
                loca[4 * cid],
                loca[4 * cid + 1],
                loca[4 * cid + 2],
                loca[4 * cid + 3],
            ]))
            .expect("a glyph offset");
            let end = usize::try_from(u32::from_be_bytes([
                loca[4 * (cid + 1)],
                loca[4 * (cid + 1) + 1],
                loca[4 * (cid + 1) + 2],
                loca[4 * (cid + 1) + 3],
            ]))
            .expect("a glyph offset");
            let data = &glyf[start..end];
            if !is_composite(data) {
                continue;
            }
            composites += 1;
            let mut at = 10;
            loop {
                let flags = word(data, at);
                let component = usize::from(word(data, at + 2));
                assert!(
                    component < count,
                    "the component's id is a CID of this subset"
                );
                at += 4;
                at += if flags & 0x0001 == 0 { 2 } else { 4 };
                at += if flags & 0x0008 != 0 {
                    2
                } else if flags & 0x0040 != 0 {
                    4
                } else if flags & 0x0080 != 0 {
                    8
                } else {
                    0
                };
                if flags & 0x0020 == 0 {
                    break;
                }
            }
        }
        assert!(composites > 0, "the requested character is composite");
    }

    #[test]
    fn the_subset_is_small_and_the_to_unicode_map_is_first_use() {
        let face = Face::bundled();
        let characters = ['Ё', 'A', 'Ё', 'B'];
        let subset = face.subset(&characters);

        let mut expected: Vec<(u16, char)> = Vec::new();
        for &character in &characters {
            let cid = subset.codes.get(&character).copied().unwrap_or(0);
            if cid != 0 && !expected.iter().any(|&(seen, _)| seen == cid) {
                expected.push((cid, character));
            }
        }
        assert_eq!(subset.unicode, expected);

        let mut seen = HashMap::new();
        for &(cid, character) in &subset.unicode {
            assert_ne!(cid, 0);
            assert_eq!(seen.insert(cid, character), None, "one entry per CID");
        }
        assert!(subset.program.len() < 20_000);
        assert_eq!(
            subset.widths.len(),
            usize::from(word(&table_of(&subset.program, "maxp"), 4))
        );
    }

    #[test]
    fn the_program_is_a_checksummed_sfnt_a_parser_accepts() {
        let face = Face::bundled();
        let subset = face.subset(&['A', 'Ё', 'é']);
        let program = &subset.program;

        let count = usize::from(u16::from_be_bytes([program[4], program[5]]));
        let mut previous = String::new();
        for index in 0..count {
            let at = 12 + 16 * index;
            let tag = String::from_utf8_lossy(&program[at..at + 4]).into_owned();
            assert!(tag > previous, "the directory is sorted by tag");

            let stated = u32::from_be_bytes([
                program[at + 4],
                program[at + 5],
                program[at + 6],
                program[at + 7],
            ]);
            let offset = u32::from_be_bytes([
                program[at + 8],
                program[at + 9],
                program[at + 10],
                program[at + 11],
            ]);
            let length = u32::from_be_bytes([
                program[at + 12],
                program[at + 13],
                program[at + 14],
                program[at + 15],
            ]);
            assert_eq!(offset % 4, 0, "a table starts on a word");
            let offset = usize::try_from(offset).expect("a table offset");
            let length = usize::try_from(length).expect("a table length");
            let mut data = program[offset..offset + length].to_vec();
            if tag == "head" {
                // The directory states `head`'s checksum with the adjustment still zero.
                data[8..12].fill(0);
            }
            assert_eq!(checksum(&data), stated);
            previous = tag;
        }
        assert_eq!(checksum(program), 0xB1B0_AFBA, "the file's own checksum");

        // Zeroing the adjustment states the sum the field corrects.
        let mut zeroed = program.clone();
        for index in 0..count {
            let at = 12 + 16 * index;
            if &zeroed[at..at + 4] == b"head" {
                let offset = usize::try_from(u32::from_be_bytes([
                    zeroed[at + 8],
                    zeroed[at + 9],
                    zeroed[at + 10],
                    zeroed[at + 11],
                ]))
                .expect("a table offset");
                zeroed[offset + 8..offset + 12].fill(0);
            }
        }
        let head = table_of(program, "head");
        let adjustment = u32::from_be_bytes([head[8], head[9], head[10], head[11]]);
        assert_eq!(
            checksum(&zeroed).wrapping_add(adjustment),
            0xB1B0_AFBA,
            "the adjustment is what the file needs"
        );
    }
}

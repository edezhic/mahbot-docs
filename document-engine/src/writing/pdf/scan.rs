//! A file's body read the way the reference's library reads it.
//!
//! The reference never consults a file's cross-reference index. It walks the body from the header,
//! object by object, and takes the last definition of an object it meets — so a file whose offsets
//! are shifted, whose index is absent, whose `startxref` points at the wrong place, or whose index
//! was not updated by an incremental save reads exactly as its body says. A stream whose stated
//! `/Length` does not land on `endstream` is recovered by scanning for that keyword, counting nested
//! `stream` keywords on the way, whatever the length states and whether it is stated at all.
//!
//! Two container shapes are part of the walk: an object stream (`/Type /ObjStm`) is decoded and the
//! objects it holds become ordinary objects, and an index stream (`/Type /XRef`) contributes only the
//! entries its dictionary states. Neither is kept as an object, which is what the reference does too.
//! An object stream the walk cannot expand is kept as the bytes it states, as the reference keeps it, so
//! the objects inside it are as unreachable as they were in the input; an index stream it cannot decode
//! is dropped, because the only thing such a stream holds is an index the writer rebuilds.
//!
//! The walk refuses what the reference's parser refuses, and that is the point of reproducing it
//! rather than repairing an index: a header, an index section, a trailer or an object header whose
//! syntax does not parse — a cross-reference entry that starts with a digit and then states something
//! else, an unterminated trailer dictionary — refuses it, even though the body is otherwise intact.
//! A missing `endobj` is not one of those: the reference matches that keyword and carries on when it
//! is absent, and so does the walk.
//!
//! An object whose *body* does not parse is carried rather than refused: the walk keeps the bytes the
//! object states, between its header and its `endobj`, and the writer copies them out verbatim, which
//! is what the reference's own parser does with a body it cannot read. The produced file is then as
//! damaged as its input and no more: the object keeps what the file stated, and a reference nothing
//! answers is left dangling rather than dropped. What the walk cannot read *at all* — the objects a
//! container states and does not place — is not in the produced file, and [`Lost`] is the one home of
//! how that is counted and of what each count means; a container it cannot expand at all, or one that
//! states no count, is carried whole, and the reference's parser reaches nothing there either. An
//! object the walk cannot find an `endobj` for is not one of those: the reference's recovery throws
//! there, so the file is refused.
//!
//! The walk's work is the file's own length — times the look allowance below for a body built of long
//! comment or digit runs, and the entries a container states for one whose entries all name one offset
//! (§3.18 of `PARSER.md`) — so no file refuses it and no file needs a pass budget:
//! its position only advances, except across a look at what comes next — which inspects at most
//! [`scan_look_bytes_max`](crate::vocabulary::EngineBounds::scan_look_bytes_max) bytes of its own and
//! leaves the position where it was — and across a stream's recovery scan, which reads to the file's
//! end. A recovery that finds its marker ends the stream where the marker begins and the walk resumes
//! past it, so the bytes it read are bytes no later recovery reads again. One that reaches the file's
//! end without balancing records the tail once — every `stream` and `endstream` token it met, the
//! nesting each left behind, and for each the token that closes it — and a recovery that begins
//! inside that tail is answered from the record without reading: by two lookups, unless it begins
//! inside a recorded token, whose bytes are not a keyword to that pass, so it reads the tail once for
//! itself while the record keeps answering what follows it. A file whose streams never end therefore
//! has its tail read once rather than once per stream.
//!
//! A container's bytes are decoded stage by stage, so a chain of expanding filters multiplies rather
//! than adds; what stops it is
//! [`container_decode_max_bytes`](crate::vocabulary::EngineBounds::container_decode_max_bytes),
//! the most a typed array holds — the same wall the reference's own decoder hits, where it keeps the
//! container as the bytes it states. Which filters count their output as they produce it, which build
//! their whole output first, and what the peak of each can be, is that bound's row in `VOCABULARY.md`.
//! A container past the ceiling is carried here too, not refused.
//!
//! The walk builds arrays and dictionaries from an explicit stack rather than by recursion, so the
//! nesting it follows costs heap rather than the process's stack. It still stops at
//! [`object_depth_max`](crate::vocabulary::EngineBounds::object_depth_max) — an object nested past it is
//! kept as its own bytes, exactly as the reference's own parser keeps an object past its stack — so a
//! file cannot make the engine hold an unbounded object graph. The bound is a memory bound, not a
//! refusal: nothing is lost by carrying the bytes, and it is set above where the reference's own parser
//! gives up, so nothing the reference parses is carried here.
//!
//! What the bound does *not* do is keep the depth away from the language's own recursion: a value
//! inside it is still built, and such a value can be thousands of levels deep, so `graph` states the
//! rule that keeps every walk off it. A trailer dictionary is built without the bound at all: it
//! states four entries and the rest is dropped where it stands.
//!
//! Shapes the object model cannot key are read anyway, because the reference reads them: an object
//! stated at a number or a generation past what `(u32, u16)` holds — and one stated at the highest
//! generation, which the walk's own markers use while the body is read — is held by the number the
//! file states as the reference keys it, the double its digits state with its generation, until the
//! body has been read, and is then given a number of the document's own: one past every number the
//! file states or names, or the lowest number neither states nor names when the file states the
//! format's ceiling, so a reference the file leaves dangling stays dangling, with every reference to
//! it following. Two runs of digits the reference reads as two numbers are two objects here, and two
//! it reads as one are one. A container the walk cannot expand is carried as the bytes it states,
//! and the objects it states are gone, which the document reports to its caller as the loss it is.
//! `PARSER.md` in the crate's root lists every such shape, one entry each, with what the reference
//! does and what this walk does.

use std::collections::hash_map::Entry;
use std::collections::{BTreeMap, HashMap, HashSet};

use flate2::{Decompress, FlushDecompress, Status};
use lopdf::{Dictionary, Object, ObjectId, Stream, StringFormat};

use super::graph::{self, Document};
use super::lzw;

use crate::vocabulary::vocabulary;
use crate::writing::whole;

/// The walk refused the file: the bytes do not form a document the reference's parser reads, a
/// shape it refuses too.
///
/// An object whose own syntax does not parse is not one of these: [`Scan::indirect_objects`] keeps it
/// as its own bytes instead, and so it does with an object nested past the depth the walk builds. This
/// escapes only from the file's own framing — a header, an index section or a trailer dictionary —
/// which no walk can carry.
#[derive(Debug)]
pub(super) struct Refusal;

/// The bounds a scan is held to, every one of them stated in the vocabulary. A test injects its own
/// so a bound can be reached by a small fixture rather than by a file built to be large.
#[derive(Clone, Copy)]
pub(super) struct Bounds {
    /// The deepest nesting of arrays and dictionaries the scan builds.
    pub(super) depth: usize,
    /// The bytes one look at what comes next may inspect.
    pub(super) look: usize,
    /// The most a container's bytes may decode to. Held as a `u64` because the ceiling is past what a
    /// 32-bit `usize` states, and a bound a host cannot name is worse than one it can compare.
    pub(super) decode: u64,
}

impl Bounds {
    /// The bounds the engine ships.
    pub(super) fn stated() -> Self {
        let engine = &vocabulary().engine;
        Self {
            depth: engine.object_depth_max,
            look: engine.scan_look_bytes_max,
            decode: engine.container_decode_max_bytes,
        }
    }
}

/// What the produced file will not hold or no longer names, and the two facts a count of it may be:
/// the objects the walk counted in a container it read, and the objects a container it could not read
/// says it holds. The second is the input's own claim rather than a count — nothing the walk read says
/// what such a container held — and the answer says so rather than printing a number of its own. An
/// answer that said nothing would claim the objects were carried.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) struct Lost {
    /// The objects the produced file does not hold as an object: the ones a container the walk read
    /// did not place, the ones a later statement of the same number replaced, and the ones the index
    /// cannot name because one number holds two bodies.
    pub(super) objects: usize,
    /// The objects a container the walk could not read states it holds. The engine counted nothing
    /// here: the number is the file's, and the answer attributes it to the file.
    pub(super) unread: usize,
    /// Whether [`Lost::unread`] is past what this host holds — a count no count of a document can be,
    /// which the answer states as that fact rather than as the ceiling standing in for it.
    pub(super) beyond: bool,
    /// The references inside a carried body that no longer name what the input named.
    pub(super) references: usize,
}

impl Lost {
    pub(super) fn add(&mut self, other: Self) {
        self.objects = self.objects.saturating_add(other.objects);
        self.beyond |= other.beyond;
        self.add_unread(other.unread);
        self.references = self.references.saturating_add(other.references);
    }

    /// Add what a copy of these objects does not carry. A copy rebuilds the numbering of what it
    /// writes, so a reference the input states is not one the produced file states: only the objects
    /// and the container counts are reported.
    pub(super) fn add_objects(&mut self, other: Self) {
        self.add(Self {
            references: 0,
            ..other
        });
    }

    /// Take the count a container the walk could not read states. A total past what this host holds is
    /// kept as the fact that it is past, never as the ceiling standing in for the file's number.
    pub(super) fn add_unread(&mut self, count: usize) {
        self.unread = self.unread.saturating_add(count);
        self.beyond |= self.unread == usize::MAX;
    }
}

/// The document the file's body states, read the way the reference reads one, and what the produced
/// file will not hold ([`Lost`] states the counts and what each one means).
pub(super) fn document(bytes: &[u8], bounds: Bounds) -> Result<(Document, Lost), Refusal> {
    let mut scan = Scan::new(bytes, bounds);
    let version = scan.header()?;
    // The reference's own loop: a section that consumed nothing would repeat for ever, so a stall is
    // a file it cannot read rather than one it retries.
    let mut stall = usize::MAX;
    while !scan.done() {
        scan.section()?;
        if scan.at == stall {
            return Err(Refusal);
        }
        stall = scan.at;
    }
    // The index states one entry per object number and none at number zero, so an object stated at
    // number zero at a generation the model holds is one the produced file cannot name: the reference
    // keeps it and writes an index that cannot name it, and the answer says so. (One at the marker
    // generation is held like a number past the model and given a number of the document's own, so
    // nothing is lost there.) It is dropped before the objects past the model are placed, so the
    // references its carried bytes state are not counted as ones a placement broke — the produced file
    // states neither. Number zero at generation zero is the free list's head rather than an object, so
    // leaving it out loses nothing.
    let mut unnamed = 0_usize;
    let zeros: Vec<ObjectId> = scan
        .objects
        .range((0, 0)..=(0, u16::MAX))
        .map(|(&id, _)| id)
        .collect();
    for id in zeros {
        if let Some(object) = scan.objects.remove(&id) {
            graph::discard(object);
        }
        if id.1 != 0 {
            unnamed = unnamed.saturating_add(1);
        }
    }
    scan.place_beyond();
    scan.recover_root();
    let mut lost = scan.numbering.lost;
    lost.objects = lost.objects.saturating_add(unnamed);
    // The map is ordered by number and then generation, so each number after the first at that number
    // is a body the index cannot name.
    let mut previous: Option<u32> = None;
    for &(number, _) in scan.objects.keys() {
        if previous == Some(number) {
            lost.objects = lost.objects.saturating_add(1);
        }
        previous = Some(number);
    }
    let mut document = Document::with_version(&version);
    document.inner.max_id = scan
        .objects
        .keys()
        .map(|&(number, _)| number)
        .max()
        .unwrap_or(0);
    document.inner.objects = std::mem::take(&mut scan.objects);
    document.inner.trailer = std::mem::take(&mut scan.trailer);
    document.named = std::mem::take(&mut scan.numbering.named);
    document.spare = scan.numbering.spare;
    Ok((document, lost))
}

/// What a recovery that read to the file's end without finding its marker learned about the rest of
/// the file: every `stream` and `endstream` token in the tail, where it begins and ends, and the
/// nesting it left behind (the scan starts at 1 and one `endstream` ends a stream). A later recovery
/// starting inside the tail is answered from this record instead of reading the tail again, so a file
/// of streams that never end costs one pass over its tail rather than the square of its length.
///
/// The record holds one token per keyword the tail states, 32 bytes each — some five times the bytes
/// those keywords take, and alive only while a walk runs — which is the price of answering without
/// reading: a file dense in `stream` keywords holds a record a few times its own length.
struct Recovery {
    from: usize,
    tokens: Vec<Token>,
}

/// One `stream` or `endstream` keyword in the recorded tail: where it begins and ends, the nesting it
/// left behind, and the first later token whose nesting is lower. The nesting changes by one per
/// token, so that token closes the level this one left open — which is what lets a later recovery be
/// answered by two lookups rather than by a search of the tail.
struct Token {
    start: usize,
    end: usize,
    nesting: i64,
    /// The index of the first later token with a lower nesting, or `usize::MAX` when none follows —
    /// a sentinel rather than `Option<usize>`, which would pad the token out to 40 bytes.
    closer: usize,
}

impl Recovery {
    /// What a scan from `at` answers, when the record can say: the marker's position and the position
    /// past its keyword — where a fresh scan would have left the walk — or a refusal because no token
    /// in the tail ends the stream. `None` when the record does not cover `at`, or says nothing about
    /// it — a token that begins before `at` and ends after it was one keyword to the pass that
    /// recorded it, and a pass beginning inside it reads other bytes there, so only a fresh scan can
    /// say what happens.
    fn answer(&self, at: usize) -> Option<Result<(usize, usize), Refusal>> {
        if at < self.from {
            return None;
        }
        // The tokens are disjoint and in the order the pass met them, so their ends are ordered too:
        // the last one ending at or before `at` left the nesting the tail has there, and one that
        // begins before `at` without having ended straddles it.
        let reached = self.tokens.partition_point(|token| token.end <= at);
        if let Some(token) = self.tokens.get(reached)
            && token.start < at
        {
            return None;
        }
        // A pass beginning at `at` is one stream in at the nesting the tail has there, and looks for
        // the token that brings it one level lower: the next token when that one closes a level,
        // otherwise the closer of the next token's closer — the nesting falls by one per token, so that
        // second closer is the level below. The pass that recorded the tail never balanced, so a level
        // of 1 there is one nothing left in the tail ends.
        let level = match reached.checked_sub(1) {
            Some(last) => self.tokens[last].nesting,
            None => 1,
        };
        let found = match self.tokens.get(reached) {
            Some(token) if token.nesting < level => Some(reached),
            Some(token) => self.tokens.get(token.closer).map(|closing| closing.closer),
            None => None,
        };
        match found.and_then(|index| self.tokens.get(index)) {
            Some(token) => Some(Ok((token.start, token.end))),
            None => Some(Err(Refusal)),
        }
    }
}

/// The scan's position in the file, the objects it has read and the trailer it has collected.
struct Scan<'a> {
    bytes: &'a [u8],
    at: usize,
    /// While a look at what comes next is in progress, the bytes it may still inspect; `None` while
    /// reading.
    looking: Option<u64>,
    /// What one recovery already learned about the rest of the file, so a later one that begins
    /// inside it is answered without reading anything.
    recovery: Option<Recovery>,
    /// The bounds the walk is held to.
    bounds: Bounds,
    objects: BTreeMap<ObjectId, Object>,
    /// Every object number in the order it was first defined. This is the only home of that order —
    /// the map is keyed by number — which is why root recovery walks it.
    order: Vec<ObjectId>,
    /// The four entries a trailer or an index stream states, the last statement of each winning.
    trailer: Dictionary,
    /// How objects are numbered, which a container's walk shares with the document it feeds.
    numbering: Numbering,
}

/// Where the two runs of digits a statement of a number begin in the bytes: the number's and — for a
/// header, which states one — its generation's.
#[derive(Clone, Copy)]
struct Runs {
    number: usize,
    generation: Option<usize>,
}

/// The value one statement of a number states, both runs of digits with the leading zeroes trimmed —
/// the value, not the spelling. Two statements of one pair are one object, the second a redefinition
/// of the first; two statements that differ in either value are two bodies, and the produced file
/// holds one of them.
#[derive(PartialEq, Eq)]
struct Digits {
    number: Vec<u8>,
    generation: Vec<u8>,
}

impl Digits {
    /// The two values a statement of a number states, from where each run of digits begins. A
    /// statement that has no generation — a container's entry table states none — has none here
    /// either, so a header stating generation zero and an entry naming the same number are one value.
    fn at(bytes: &[u8], runs: Runs) -> Self {
        Self {
            number: trim(digits_at(bytes, runs.number)),
            generation: runs
                .generation
                .map_or_else(Vec::new, |at| trim(digits_at(bytes, at))),
        }
    }
}

/// A number a stream's dictionary states, in the two shapes the object model holds one: a whole
/// number, or the real a file's own digits make.
#[derive(Clone, Copy)]
enum Numeric {
    Whole(i64),
    Real(f32),
}

/// A run of digits without the leading zeroes it may carry.
fn trim(digits: &[u8]) -> Vec<u8> {
    let start = digits
        .iter()
        .position(|byte| *byte != b'0')
        .unwrap_or(digits.len());
    digits[start..].to_vec()
}

/// How the walk keys one object number.
#[derive(Clone, Copy, Debug)]
enum Key {
    /// A pair the object model holds.
    Held(ObjectId),
    /// A number it cannot hold, kept by the number the file states as the reference reads it — the
    /// double its digits state, with its generation — under the marker every reference to it is
    /// written as, until [`Scan::place_beyond`] gives it a number of the document's own.
    Beyond { pair: (u64, u64), marker: ObjectId },
}

impl Key {
    /// The id a reference to this number is written as.
    fn marker(self) -> ObjectId {
        match self {
            Key::Held(id) => id,
            Key::Beyond { marker, .. } => marker,
        }
    }
}

/// Everything the walk knows about object numbers, held apart so a container's walk can share it.
struct Numbering {
    /// The objects the file states that the object model cannot key, each with the value the file
    /// stated for it. They are held by the pair the file states until the whole body has been read,
    /// because the number a produced file may give one has to be one the file neither states nor
    /// names, and that is only known then — see [`Scan::place_beyond`]. Every entry is a pair
    /// `holders` answers for.
    ///
    /// The key is the number the way the reference keys it — the double each run of digits states, with
    /// its generation — so two runs that round to one double are one number to this map, as they are to
    /// the reference, and two that do not are two. The values are kept because a second statement of
    /// one pair is a redefinition only when it states the same pair again: another value is a body the
    /// produced file no longer holds.
    beyond: HashMap<(u64, u64), (Digits, Object)>,
    /// The marker each such pair is keyed by while the walk runs, and that every reference to it is
    /// written as, so the reference follows the object to the number it is given at the end; with the
    /// id the file states for the pair, where the object model can hold that id — which is where a
    /// reference to a pair no object states is written back. A marker states the format's own highest
    /// generation, which is why no object the file states is ever keyed by one: a pair at that
    /// generation is held here like one past the model.
    holders: HashMap<(u64, u64), (ObjectId, Option<ObjectId>)>,
    /// The next marker, counting down from the format's ceiling. Distinct pairs get distinct markers,
    /// so two numbers the reference keys apart never merge — and a file that states or names a
    /// marker's number is a pair this map holds, so the file's own meaning is the one the map answers.
    next_marker: u32,
    /// Every object number the file names in a reference. A number given to an object the model
    /// cannot key is one the file neither states nor names, so a reference it leaves dangling keeps
    /// naming nothing.
    named: HashSet<u32>,
    /// The highest number the file states or names, and the highest this walk has given an object. A
    /// number given to an object the model cannot key is one past it, so the search for one starts
    /// there rather than at the beginning.
    highest: u32,
    /// Where the search for a free number resumes: every number below it has been looked at.
    spare: u32,
    /// What the produced file will not hold or no longer name: the one home of the count the answer
    /// reports, filled by [`Scan::assign`] where a replacement happens, by
    /// [`Scan::object_stream`] where a container is not carried whole, and by
    /// [`Scan::place_beyond`] where a placement breaks a reference.
    lost: Lost,
}

impl Numbering {
    fn new() -> Self {
        Self {
            beyond: HashMap::new(),
            holders: HashMap::new(),
            next_marker: u32::MAX,
            named: HashSet::new(),
            highest: 0,
            spare: 1,
            lost: Lost::default(),
        }
    }
}

impl Drop for Numbering {
    fn drop(&mut self) {
        // A walk that stops part-way holds objects a file may have nested as deep as the scan builds
        // one, so they are dismantled here rather than by the language's own drop.
        graph::discard_all(self.beyond.drain().map(|(_, (_, object))| object));
    }
}

/// A container the parser is still filling: the values read so far and, for a dictionary, the key the
/// next value belongs to.
enum Frame {
    Array(Vec<Object>),
    Dictionary(Dictionary, Option<Vec<u8>>),
}

/// Dismantle the containers a walk gave up on from one worklist, rather than one stack frame per
/// level: a trailer may be nested as deep as the file states.
fn discard_frames(stack: &mut Vec<Frame>) {
    let mut partial: Vec<Object> = Vec::new();
    for frame in stack.drain(..) {
        match frame {
            Frame::Array(values) => partial.push(Object::Array(values)),
            Frame::Dictionary(dictionary, _) => partial.push(Object::Dictionary(dictionary)),
        }
    }
    graph::discard_all(partial);
}

/// Dismantle one frame a walk gave up on, on the heap for the same reason.
fn discard_frame(frame: Frame) {
    discard_frames(&mut vec![frame]);
}

/// The refusal a container nested past the depth the walk builds answers with. It is the same refusal a
/// body that does not parse gives, because the caller answers both the same way: the object is kept as
/// the bytes it states.
fn too_deep() -> Refusal {
    tracing::debug!(
        "document: an object nests deeper than the walk builds and is kept as its bytes"
    );
    Refusal
}

impl Drop for Scan<'_> {
    fn drop(&mut self) {
        // A walk that stops part-way holds objects and a trailer built from a file that may nest them
        // as deep as the scan builds one, so they are dismantled here rather than by the language.
        let pending = std::mem::take(&mut self.objects).into_values();
        graph::discard_all(pending.chain([Object::Dictionary(std::mem::take(&mut self.trailer))]));
    }
}

impl<'a> Scan<'a> {
    fn new(bytes: &'a [u8], bounds: Bounds) -> Self {
        Self {
            bytes,
            at: 0,
            looking: None,
            recovery: None,
            bounds,
            objects: BTreeMap::new(),
            order: Vec::new(),
            trailer: Dictionary::new(),
            numbering: Numbering::new(),
        }
    }

    // ── the bytes ───────────────────────────────────────────────────────────────────────────────

    fn done(&self) -> bool {
        self.at >= self.bytes.len()
    }

    fn peek(&self) -> Option<u8> {
        self.bytes.get(self.at).copied()
    }

    fn ahead(&self, steps: usize) -> Option<u8> {
        self.bytes.get(self.at + steps).copied()
    }

    fn next(&mut self) -> Result<u8, Refusal> {
        let Some(&byte) = self.bytes.get(self.at) else {
            return Err(Refusal);
        };
        self.charge(1)?;
        self.at += 1;
        Ok(byte)
    }

    /// Consume `keyword` when the bytes state it. A spelling the bytes do not state is free: what the
    /// bound counts is how much of the file the scan reads, not how many spellings it tests at each
    /// byte.
    fn keyword(&mut self, keyword: &[u8]) -> Result<bool, Refusal> {
        if !self.bytes[self.at..].starts_with(keyword) {
            return Ok(false);
        }
        self.at += keyword.len();
        self.charge(u64::try_from(keyword.len()).unwrap_or(u64::MAX))?;
        Ok(true)
    }

    /// Whether `test` finds what it looks for at the scan's position, leaving the scan where it was.
    /// A look is not a read: it inspects at most its own look allowance and never the file's, and the
    /// section that consumes what the look found charges it from its own start. What a look must not
    /// do is state anything about an object's numbering, which is why it asks
    /// [`object_header`](Self::object_header) rather than the numbered header: a look that gave a
    /// number to what it saw would take that number out of the document's hands.
    fn look(&mut self, test: impl FnOnce(&mut Self) -> Result<bool, Refusal>) -> bool {
        let at = self.at;
        let looking = self.looking.replace(self.bounds.look as u64);
        let found = test(self).is_ok_and(|found| found);
        self.looking = looking;
        self.seek(at);
        found
    }

    /// Move the scan to `at` without charging. The bound counts the bytes the scan reads, not the
    /// distance between two places it reads from: a stated stream length is a place to look at, and
    /// what the scan reads at that place is what it charges.
    fn seek(&mut self, at: usize) {
        self.at = at;
    }

    /// Charge `bytes` against the look allowance in force, if a look is: a read outside one costs
    /// nothing, because the look's allowance is the only budget this walk keeps. Running out of it is
    /// not a refusal — [`Self::look`] reads every error as "found nothing".
    fn charge(&mut self, bytes: u64) -> Result<(), Refusal> {
        if let Some(left) = &mut self.looking {
            *left = left.checked_sub(bytes).ok_or(Refusal)?;
        }
        Ok(())
    }

    fn skip_space(&mut self) -> Result<(), Refusal> {
        while self.peek().is_some_and(is_space) {
            self.next()?;
        }
        Ok(())
    }

    /// A comment runs to the end of its line, which is left for the whitespace pass.
    fn skip_comment(&mut self) -> Result<bool, Refusal> {
        if self.peek() != Some(b'%') {
            return Ok(false);
        }
        while let Some(byte) = self.peek() {
            if byte == b'\n' || byte == b'\r' {
                return Ok(true);
            }
            self.next()?;
        }
        Ok(true)
    }

    fn skip_space_and_comments(&mut self) -> Result<(), Refusal> {
        self.skip_space()?;
        while self.skip_comment()? {
            self.skip_space()?;
        }
        Ok(())
    }

    fn assert_next(&mut self, expected: u8) -> Result<(), Refusal> {
        if self.peek() != Some(expected) {
            return Err(Refusal);
        }
        self.next()?;
        Ok(())
    }

    // ── the document ────────────────────────────────────────────────────────────────────────────

    /// The version the header states. The reference finds the header anywhere in the file, so a
    /// file with leading bytes is still a file.
    fn header(&mut self) -> Result<String, Refusal> {
        while !self.done() {
            if self.keyword(b"%PDF-")? {
                let major = self.raw_int()?;
                self.assert_next(b'.')?;
                let minor = self.raw_int()?;
                self.skip_binary_header_comment()?;
                return Ok(format!("{major}.{minor}"));
            }
            self.next()?;
        }
        Err(Refusal)
    }

    /// The bytes after the header line are a binary mark, not an object header. The reference looks
    /// for a header there and, finding none, steps one byte past it.
    fn skip_binary_header_comment(&mut self) -> Result<(), Refusal> {
        self.skip_space_and_comments()?;
        if !self.look(|scan| scan.object_header().map(|_| true)) {
            self.next()?;
            self.skip_space_and_comments()?;
        }
        Ok(())
    }

    /// One pass of the reference's loop over the body: the objects, then the index section a file
    /// may state, then whatever lies between.
    fn section(&mut self) -> Result<(), Refusal> {
        self.indirect_objects()?;
        self.cross_reference_section()?;
        self.trailer_dictionary()?;
        self.trailer_offset()?;
        self.skip_jibberish()
    }

    fn indirect_objects(&mut self) -> Result<(), Refusal> {
        self.skip_space_and_comments()?;
        while self.peek().is_some_and(is_digit) {
            let start = self.at;
            // A body that does not parse is kept as the bytes it states, which is what the
            // reference's parser does with one it cannot read — a body nested past the depth the
            // engine builds included.
            if self.indirect_object().is_err() {
                self.seek(start);
                self.unreadable_object()?;
            }
            self.skip_space_and_comments()?;
            self.skip_jibberish()?;
        }
        Ok(())
    }

    fn indirect_object(&mut self) -> Result<(), Refusal> {
        let (key, runs) = self.indirect_object_header()?;
        self.skip_space_and_comments()?;
        let object = self.object(self.bounds.depth);
        self.skip_space_and_comments()?;
        let _ = self.keyword(b"endobj")?;
        let object = object?;
        let (objects_stream, index_stream) = match &object {
            Object::Stream(stream) => {
                let kind = resolved_type(&stream.dict, &self.objects);
                (
                    kind == Some(b"ObjStm".as_slice()),
                    kind == Some(b"XRef".as_slice()),
                )
            }
            _ => (false, false),
        };
        let handled = if let Object::Stream(stream) = &object {
            if objects_stream {
                // A container is expanded into the objects it holds rather than kept. One the walk
                // cannot expand stays the bytes it states, like any other object it cannot read, and
                // the objects it held are reported as the loss they are.
                Some(self.object_stream(stream))
            } else if index_stream {
                // The reference's parser never assigns an XRef stream it could read, so neither does
                // this walk; one it cannot read is an object it could not parse, and that is carried.
                Some(self.index_stream(stream).then_some(()).ok_or(Refusal))
            } else {
                None
            }
        } else {
            None
        };
        if let Some(outcome) = handled {
            // The object itself is not kept, so it is dismantled here rather than by the language's
            // own drop: a container may hold a value nested as deep as the scan builds one.
            graph::discard(object);
            outcome
        } else {
            self.assign(key, self.bytes, runs, object);
            Ok(())
        }
    }

    /// An object the walk cannot read: it is kept as the bytes it states, between its header and its
    /// `endobj`, and the writer copies those bytes out verbatim. That is what the reference's parser
    /// does with a body it cannot read, so the produced file is as damaged as its input and no more.
    /// A body with no `endobj` is not one of these: the reference's recovery throws on it, so the file
    /// is refused.
    ///
    /// The start position marks the object as one whose bytes were never parsed; nothing else in a
    /// document read from a body carries one.
    fn unreadable_object(&mut self) -> Result<(), Refusal> {
        let (key, runs) = self.indirect_object_header()?;
        self.skip_space_and_comments()?;
        let start = self.at;
        loop {
            if self.done() {
                return Err(Refusal);
            }
            if self.keyword(b"endobj")? {
                break;
            }
            self.next()?;
        }
        let end = self.at - b"endobj".len();
        tracing::debug!(
            ?key,
            bytes = end.saturating_sub(start),
            "document: an object could not be read and is carried as its own bytes"
        );
        let mut stream = Stream::new(Dictionary::new(), self.bytes[start..end].to_vec());
        stream.start_position = Some(start);
        self.assign(key, self.bytes, runs, Object::Stream(stream));
        Ok(())
    }

    /// The key a header's numbers give it, and where the runs of digits it spells begin: the values
    /// those runs state are what tells a redefinition of one object from a statement of another that
    /// keys as it, and only a number the model cannot key is compared with anything.
    fn indirect_object_header(&mut self) -> Result<(Key, Runs), Refusal> {
        let (number, generation, runs) = self.object_header()?;
        Ok((self.stated(number, generation, runs), runs))
    }

    /// The two numbers and the keyword a header states, before anything is numbered: what a look at
    /// what comes next asks, since a look states nothing. The two positions are the first digit of
    /// each run, which is where the number is read from once it is keyed.
    fn object_header(&mut self) -> Result<(u64, u64, Runs), Refusal> {
        self.skip_space_and_comments()?;
        let number_at = self.at;
        let number = self.raw_int()?;
        self.skip_space_and_comments()?;
        let generation_at = self.at;
        let generation = self.raw_int()?;
        self.skip_space_and_comments()?;
        if !self.keyword(b"obj")? {
            return Err(Refusal);
        }
        let runs = Runs {
            number: number_at,
            generation: Some(generation_at),
        };
        Ok((number, generation, runs))
    }

    /// The index section, parsed for its syntax alone: its entries are never used, and a section
    /// that does not parse refuses the file exactly as it refuses the reference.
    fn cross_reference_section(&mut self) -> Result<(), Refusal> {
        self.skip_space_and_comments()?;
        if !self.keyword(b"xref")? {
            return Ok(());
        }
        self.skip_space_and_comments()?;
        while self.peek().is_some_and(is_digit) {
            let _ = self.raw_int()?;
            self.skip_space_and_comments()?;
            let _ = self.raw_int()?;
            self.skip_space_and_comments()?;
            if matches!(self.peek(), Some(b'n' | b'f')) {
                self.next()?;
            }
            self.skip_space_and_comments()?;
        }
        Ok(())
    }

    fn trailer_dictionary(&mut self) -> Result<(), Refusal> {
        self.skip_space_and_comments()?;
        if !self.keyword(b"trailer")? {
            return Ok(());
        }
        self.skip_space_and_comments()?;
        let dictionary = self.dictionary()?;
        self.adopt_trailer(&dictionary);
        // Only the four entries a trailer states are kept, so the rest — which a file may nest to any
        // depth — is dismantled here rather than by the language's own drop.
        graph::discard(Object::Dictionary(dictionary));
        Ok(())
    }

    /// `startxref` and the two end-of-file marks, whose values the walk does not need: the index is
    /// never followed, but its syntax still has to parse.
    fn trailer_offset(&mut self) -> Result<(), Refusal> {
        self.skip_space_and_comments()?;
        if !self.keyword(b"startxref")? {
            return Ok(());
        }
        self.skip_space_and_comments()?;
        let _ = self.raw_int()?;
        self.skip_space()?;
        let _ = self.keyword(b"%%EOF")?;
        self.skip_space_and_comments()?;
        let _ = self.keyword(b"%%EOF")?;
        self.skip_space_and_comments()
    }

    /// Whatever is not an object, an index section or a trailer: the reference walks past it until
    /// the next thing it recognises begins.
    fn skip_jibberish(&mut self) -> Result<(), Refusal> {
        self.skip_space_and_comments()?;
        while !self.done() {
            let byte = self.peek().unwrap_or_default();
            let begins = (b' '..=b'~').contains(&byte)
                && self.look(|scan| {
                    Ok(scan.keyword(b"xref")?
                        || scan.keyword(b"trailer")?
                        || scan.keyword(b"startxref")?
                        || scan.object_header().is_ok())
                });
            if begins {
                break;
            }
            self.next()?;
        }
        Ok(())
    }

    /// Take the entries a `trailer` dictionary states, leaving an entry it does not state as an
    /// earlier statement left it.
    fn adopt_trailer(&mut self, dictionary: &Dictionary) {
        for key in TRAILER_KEYS {
            if let Ok(value) = dictionary.get(key) {
                graph::set(&mut self.trailer, *key, graph::clone(value));
            }
        }
    }

    /// Take the entries an index stream's dictionary states, replacing the trailer: an entry it
    /// leaves out is no longer stated at all.
    fn replace_trailer(&mut self, dictionary: &Dictionary) {
        for key in TRAILER_KEYS {
            match dictionary.get(key) {
                Ok(value) => graph::set(&mut self.trailer, *key, graph::clone(value)),
                Err(_) => {
                    if let Some(removed) = self.trailer.remove(key) {
                        graph::discard(removed);
                    }
                }
            }
        }
    }

    /// The catalog the document's root names, or — when the root does not name one — the last
    /// catalog the body defines, which is how the reference recovers a file whose index is gone. A
    /// root that does resolve to a catalog is kept, however many other catalogs the body holds; an
    /// inline one is given a number of its own, since this model reaches a catalog only through a
    /// reference.
    fn recover_root(&mut self) {
        let root = self
            .trailer
            .get(b"Root")
            .ok()
            .filter(|root| is_catalog(root, &self.objects))
            .map(graph::clone)
            .or_else(|| {
                let id = self.order.iter().rev().copied().find(|id| {
                    self.objects
                        .get(id)
                        .is_some_and(|object| is_catalog(object, &self.objects))
                })?;
                tracing::debug!(?id, "document: recovered the catalog from the body");
                let root = Object::Reference(id);
                graph::set(&mut self.trailer, b"Root", root.clone());
                Some(root)
            });
        let Some(root) = root else {
            return;
        };
        let Object::Dictionary(catalog) = root else {
            graph::discard(root);
            return;
        };
        // One past the highest number the file states or names, so a reference the file left dangling
        // keeps naming nothing; `spare_number` falls back to the lowest free number when the file
        // states the ceiling itself.
        let id = (self.spare_number(), 0);
        tracing::debug!(?id, "document: gave an inline catalog a number of its own");
        self.put(id, Object::Dictionary(catalog));
        graph::set(&mut self.trailer, b"Root", Object::Reference(id));
    }

    /// Put an object under the key the walk gave its number: a pair the model holds, or one it does
    /// not, which is kept by the number the file states until [`Scan::place_beyond`] gives it a number.
    ///
    /// This is the one place where a statement of a number the model cannot key meets the statement
    /// before it, so it is where a replacement is counted — the count happens here and nowhere else,
    /// whatever the second statement's body turned out to be. Two statements of one pair are one
    /// object: the same value twice is a redefinition and loses nothing, while two values the
    /// reference reads as one number are two bodies the input states and the produced file holds one
    /// of — the object the earlier statement put there is not in it, and the answer says so.
    ///
    /// `bytes` and `runs` say where the statement's digits lie, in whatever the statement was read
    /// from: the file for a header, the decoded bytes for a container's entry. They are read only for
    /// a number the model cannot key.
    fn assign(&mut self, key: Key, bytes: &[u8], runs: Runs, object: Object) {
        match key {
            Key::Held(id) => self.put(id, object),
            Key::Beyond { pair, .. } => {
                let digits = Digits::at(bytes, runs);
                match self.numbering.beyond.entry(pair) {
                    Entry::Vacant(slot) => {
                        slot.insert((digits, object));
                    }
                    Entry::Occupied(mut slot) => {
                        // The statement before this one is dismantled here, because the produced file
                        // holds the later body for that number — and only a *different* value is an
                        // object lost.
                        let redefined = slot.get().0 == digits;
                        let (_, replaced) = slot.insert((digits, object));
                        if !redefined {
                            self.numbering.lost.objects =
                                self.numbering.lost.objects.saturating_add(1);
                        }
                        graph::discard(replaced);
                    }
                }
            }
        }
    }

    /// Put an object in the document under a number of its own.
    fn put(&mut self, id: ObjectId, object: Object) {
        // The number is now one the produced file states, so the next number given to an object the
        // model cannot key is one past it rather than a rescan of the same range.
        self.numbering.highest = self.numbering.highest.max(id.0);
        match self.objects.insert(id, object) {
            // An object stated twice replaces one a file may have nested as deep as the scan builds one.
            Some(replaced) => graph::discard(replaced),
            None => self.order.push(id),
        }
    }

    /// How this file's statement of an object number and a generation is keyed. The format puts no
    /// ceiling on either half and the object model keys `(u32, u16)`, so a number past either is kept
    /// until the whole body has been read — [`Scan::place_beyond`] then gives it a number of the
    /// document's own — and every reference to it is written as that number's marker, so the reference
    /// follows the object wherever the number it is given puts it.
    ///
    /// Such a number is keyed the way the reference keys it, by the runs of digits the positions start:
    /// the reference reads both with `Number`, so the number and the generation are doubles there, and
    /// two runs of either that round to one double are one pair to it while two that do not are two.
    /// [`double`] states the same reading here, so a file the reference keeps two objects of keeps two
    /// here too.
    ///
    /// A pair at the marker generation is kept the same way, though the model holds it: markers state
    /// that generation, so holding the file's own object there would leave one id meaning two
    /// objects. Nothing is lost by it — the object is written at a number of the document's own, as
    /// the model cannot key the file's number anyway.
    fn stated(&mut self, number: u64, generation: u64, runs: Runs) -> Key {
        // One reading of each conversion: the pair the model holds is the same pair written back if a
        // reference to it has to be restored, so both answers come from these.
        let held = u32::try_from(number).ok();
        let held_generation = u16::try_from(generation).ok();
        if let Some(number) = held {
            self.numbering.highest = self.numbering.highest.max(number);
        }
        if let (Some(number), Some(generation)) = (held, held_generation)
            && generation != u16::MAX
        {
            return Key::Held((number, generation));
        }
        let stated = held.zip(held_generation);
        let number = double(self.digits(runs.number), number);
        let generation = double(
            runs.generation.map_or(&[][..], |at| self.digits(at)),
            generation,
        );
        let pair = (number, generation);
        if let Some(&(marker, _)) = self.numbering.holders.get(&pair) {
            return Key::Beyond { pair, marker };
        }
        let marker = (self.numbering.next_marker, u16::MAX);
        self.numbering.next_marker = self.numbering.next_marker.saturating_sub(1);
        self.numbering.holders.insert(pair, (marker, stated));
        Key::Beyond { pair, marker }
    }

    /// The run of digits a number starts with at `at`, in this file.
    fn digits(&self, at: usize) -> &'a [u8] {
        digits_at(self.bytes, at)
    }

    /// Give every object the model cannot key a number of this document's own, and make the references
    /// that name it follow. The number is one the file neither states nor names — one past the highest
    /// it does, or the lowest free one when the file states that — so a reference the file leaves
    /// dangling keeps naming nothing. A number the file only names, with no object stated at it, is a
    /// reference to nothing: it is written back at the id the file used, where the model can hold that
    /// id, so the produced file leaves it where the input left it rather than at a marker's number.
    fn place_beyond(&mut self) {
        // The references a carried body states are references this walk cannot rewrite: its bytes are
        // written back verbatim. Their numbers must therefore stay free — a number one of them names
        // must not be given to an object this engine invents — and they are the only record of what
        // such a reference named, so the answer can say when a placed object left one pointing at
        // nothing. One pass over the carried bytes says which numbers to keep free, and a second, after
        // placement, counts the references it broke: a body dense in references costs its own bytes
        // twice rather than one copy of every pair it holds.
        {
            let objects = &self.objects;
            let beyond = &self.numbering.beyond;
            let named = &mut self.numbering.named;
            for bytes in carried_bodies(objects.values())
                .chain(carried_bodies(beyond.values().map(|(_, object)| object)))
            {
                carried_references(bytes, &mut |stated| {
                    if let Ok(number) = u32::try_from(stated.number) {
                        named.insert(number);
                    }
                });
            }
        }
        let mut holders: Vec<_> = self.numbering.holders.drain().collect();
        // The map's order varies from one run to the next, and so would the numbers handed out below:
        // sorted, the produced file is the same on every run.
        holders.sort_unstable();
        let mut renames: HashMap<ObjectId, ObjectId> = HashMap::new();
        let mut placed: HashSet<(u64, u64)> = HashSet::new();
        for (pair, (marker, stated)) in holders {
            let Some((_, object)) = self.numbering.beyond.remove(&pair) else {
                // The file names this number without stating an object at it, so a reference to it is
                // the dangling reference the file stated. Written back at the id the file used, where
                // the model can hold that id, so the produced file leaves it where the input left it —
                // never at a marker's number, which is one the file may not state at all.
                if let Some(stated) = stated {
                    renames.insert(marker, stated);
                }
                continue;
            };
            let id = (self.spare_number(), 0);
            tracing::debug!(
                ?pair,
                ?id,
                "document: gave an object past the model a number"
            );
            self.put(id, object);
            renames.insert(marker, id);
            placed.insert(pair);
        }
        let mut broken = 0_usize;
        for bytes in carried_bodies(self.objects.values()) {
            carried_references(bytes, &mut |stated| {
                if placed.contains(&(stated.key, stated.key_generation)) {
                    broken = broken.saturating_add(1);
                }
            });
        }
        self.numbering.lost.references = self.numbering.lost.references.saturating_add(broken);
        if renames.is_empty() {
            return;
        }
        // Only an object that states a marker is rebuilt: renaming copies a stream's content, and a
        // file may hold one object past the model among a hundred thousand that name nothing.
        let touched: Vec<ObjectId> = self
            .objects
            .iter()
            .filter(|(_, object)| graph::names_any(object, &renames))
            .map(|(id, _)| *id)
            .collect();
        for id in touched {
            let Some(object) = self.objects.remove(&id) else {
                continue;
            };
            let followed = graph::rename(&object, &renames);
            graph::discard(object);
            self.objects.insert(id, followed);
        }
        let mut trailer = Dictionary::new();
        for (key, value) in std::mem::take(&mut self.trailer) {
            graph::set(&mut trailer, key, graph::rename(&value, &renames));
            graph::discard(value);
        }
        self.trailer = trailer;
    }

    /// A number this document does not hold at any generation and that no reference in the file names:
    /// the one past the highest number the file states or names — or past the last this walk handed
    /// out — so a reference the file leaves dangling stays dangling. When the file states the format's
    /// own ceiling, the lowest free number that no reference names is taken instead.
    fn spare_number(&mut self) -> u32 {
        graph::free_number(
            &self.objects,
            &self.numbering.named,
            self.numbering.highest,
            &mut self.numbering.spare,
        )
    }

    /// Exchange the numbering with another walk: what a container's own walk is handed before it reads
    /// the objects it holds, and hands back when it is done. It is its own inverse, and the two walks
    /// must not both be reading while it is in force — which is why the container's walk is the only
    /// caller and it runs alone.
    fn share_numbering(&mut self, other: &mut Scan<'_>) {
        std::mem::swap(&mut self.numbering, &mut other.numbering);
    }

    // ── the objects ─────────────────────────────────────────────────────────────────────────────

    /// One object's own syntax, built on `stack` rather than by recursing: a file may nest an object
    /// tens of thousands deep, and the process's stack would not hold it. A container nested past
    /// `depth` stops the walk with [`Refusal`], which [`Scan::indirect_objects`] answers by
    /// keeping the object as the bytes it states — so nothing the scan builds is deeper than `depth`,
    /// and no walk over the document afterwards can recurse deeper than that either.
    fn object(&mut self, depth: usize) -> Result<Object, Refusal> {
        let mut stack: Vec<Frame> = Vec::new();
        loop {
            match self.step(&mut stack, depth) {
                Ok(Some(value)) => return Ok(value),
                Ok(None) => {}
                Err(refusal) => {
                    // What the walk built so far is nested as deep as the file states, so it is
                    // dismantled here rather than by the language's own drop.
                    discard_frames(&mut stack);
                    return Err(refusal);
                }
            }
        }
    }

    /// One value of an object being built, `Some` once the outermost container closes.
    fn step(&mut self, stack: &mut Vec<Frame>, depth: usize) -> Result<Option<Object>, Refusal> {
        self.skip_space_and_comments()?;
        // A dictionary states a key before each of its values.
        if matches!(stack.last(), Some(Frame::Dictionary(_, None))) && self.peek() != Some(b'>') {
            let key = self.name()?;
            if let Some(Frame::Dictionary(_, slot)) = stack.last_mut() {
                *slot = Some(key);
            }
            return Ok(None);
        }
        let value = if self.peek() == Some(b'[') {
            self.next()?;
            if stack.len() + 1 > depth {
                return Err(too_deep());
            }
            stack.push(Frame::Array(Vec::new()));
            return Ok(None);
        } else if self.peek() == Some(b'<') && self.ahead(1) == Some(b'<') {
            self.next()?;
            self.next()?;
            if stack.len() + 1 > depth {
                return Err(too_deep());
            }
            stack.push(Frame::Dictionary(Dictionary::new(), None));
            return Ok(None);
        } else if self.peek() == Some(b'>') && self.ahead(1) == Some(b'>') {
            self.next()?;
            self.next()?;
            let Some(frame) = stack.pop() else {
                return Err(Refusal);
            };
            let Frame::Dictionary(dictionary, slot) = frame else {
                // A closer that does not match what it closes: the frame it popped is a value the
                // file states, so it is dismantled here rather than by the language's own drop.
                discard_frame(frame);
                return Err(Refusal);
            };
            // A dictionary whose last key states no value is not one the reference's parser reads:
            // it keeps the object's bytes instead, so the walk stops here and lets the caller do the
            // same.
            if slot.is_some() {
                graph::discard(Object::Dictionary(dictionary));
                return Err(Refusal);
            }
            self.dictionary_or_stream(dictionary)?
        } else if self.peek() == Some(b']') {
            self.next()?;
            let Some(frame) = stack.pop() else {
                return Err(Refusal);
            };
            let Frame::Array(values) = frame else {
                discard_frame(frame);
                return Err(Refusal);
            };
            Object::Array(values)
        } else if self.peek() == Some(b'<') {
            self.hexadecimal_string()?
        } else if self.peek() == Some(b'(') {
            self.literal_string()?
        } else if self.peek() == Some(b'/') {
            Object::Name(self.name()?)
        } else if self.keyword(b"true")? {
            Object::Boolean(true)
        } else if self.keyword(b"false")? {
            Object::Boolean(false)
        } else if self.keyword(b"null")? {
            Object::Null
        } else if self.peek().is_some_and(is_numeric) {
            self.number_or_reference()?
        } else {
            return Err(Refusal);
        };
        match stack.last_mut() {
            None => return Ok(Some(value)),
            Some(Frame::Array(values)) => values.push(value),
            Some(Frame::Dictionary(dictionary, slot)) => {
                let Some(key) = slot.take() else {
                    // A value under no key is a dictionary the walk cannot read, and what it holds is
                    // nested as deep as the file states: it is dismantled here rather than by the
                    // language's own drop, and the frames it sits in are dismantled by the caller.
                    graph::discard(value);
                    return Err(Refusal);
                };
                // A body may state the same key twice, and the value it states first may be nested as
                // deep as the scan builds one.
                graph::set(dictionary, key, value);
            }
        }
        Ok(None)
    }

    fn number_or_reference(&mut self) -> Result<Object, Refusal> {
        let at = self.at;
        let first = self.number()?;
        self.skip_space_and_comments()?;
        let lookahead = self.at;
        if self.peek().is_some_and(is_digit) {
            let second = self.number()?;
            self.skip_space_and_comments()?;
            if self.peek() == Some(b'R') {
                self.next()?;
                let (Object::Integer(number), Object::Integer(generation)) = (&first, &second)
                else {
                    return Err(Refusal);
                };
                // A reference states two whole numbers; one this engine cannot read as a number at all
                // is not a reference it can follow, so the body is carried as the bytes it states.
                let (Ok(number), Ok(generation)) =
                    (u64::try_from(*number), u64::try_from(*generation))
                else {
                    return Err(Refusal);
                };
                // A reference names a number the file states or one it does not: the first must not be
                // given to an object the model cannot key, so every number a reference names is
                // remembered and a number handed out is never one of them.
                if let Ok(number) = u32::try_from(number) {
                    self.numbering.named.insert(number);
                }
                let runs = Runs {
                    number: at,
                    generation: Some(lookahead),
                };
                return Ok(Object::Reference(
                    self.stated(number, generation, runs).marker(),
                ));
            }
        }
        self.seek(lookahead);
        Ok(first)
    }

    /// One number, in the token the format allows: a sign, digits, and a fractional part.
    fn number(&mut self) -> Result<Object, Refusal> {
        let start = self.at;
        let mut fractional = false;
        while let Some(byte) = self.peek() {
            if !is_numeric(byte) {
                break;
            }
            self.next()?;
            if byte == b'.' {
                fractional = true;
                break;
            }
        }
        while self.peek().is_some_and(is_digit) {
            self.next()?;
        }
        let text = std::str::from_utf8(&self.bytes[start..self.at]).map_err(|_| Refusal)?;
        if text.is_empty() {
            return Err(Refusal);
        }
        if fractional {
            text.parse::<f32>().map(Object::Real).map_err(|_| Refusal)
        } else {
            text.parse::<i64>()
                .map(Object::Integer)
                .map_err(|_| Refusal)
        }
    }

    /// A whole number, as a section's counts and offsets state them. A run of digits longer than a
    /// `u64` holds saturates rather than failing: the reference reads such a run as a number too, and
    /// an object numbered past what this model keys is given a number of its own rather than refused.
    fn raw_int(&mut self) -> Result<u64, Refusal> {
        let mut value = 0_u64;
        let mut digits = 0;
        while self.peek().is_some_and(is_digit) {
            value = shift(value, self.next()?);
            digits += 1;
        }
        if digits == 0 {
            return Err(Refusal);
        }
        Ok(value)
    }

    /// A name's bytes as the file spells them, with the `#xx` escapes the reference decodes: a name
    /// the file writes as `/Ty#70e` is the `/Type` the file means.
    fn name(&mut self) -> Result<Vec<u8>, Refusal> {
        self.assert_next(b'/')?;
        let mut name = Vec::new();
        while let Some(byte) = self.peek() {
            if is_space(byte) || is_delimiter(byte) {
                break;
            }
            name.push(byte);
            self.next()?;
        }
        Ok(decode_name(name))
    }

    /// A dictionary, as a trailer states one. It is the one value built without the depth bound: a
    /// trailer states four entries and the rest is dropped where it stands, so its nesting costs heap
    /// and is never recursed on, and a file the reference reads is not refused for it.
    fn dictionary(&mut self) -> Result<Dictionary, Refusal> {
        match self.object(usize::MAX)? {
            Object::Dictionary(dictionary) => Ok(dictionary),
            other => {
                graph::discard(other);
                Err(Refusal)
            }
        }
    }

    fn hexadecimal_string(&mut self) -> Result<Object, Refusal> {
        self.assert_next(b'<')?;
        let mut bytes = Vec::new();
        let mut high: Option<u8> = None;
        while let Some(byte) = self.peek() {
            self.next()?;
            if byte == b'>' {
                // An odd final digit is padded with a zero, as the reference pads it.
                if let Some(high) = high {
                    bytes.push(high);
                }
                return Ok(Object::String(bytes, StringFormat::Hexadecimal));
            }
            if is_space(byte) {
                continue;
            }
            let Some(value) = hex_value(byte) else {
                continue;
            };
            match high.take() {
                Some(high) => bytes.push(high | value),
                None => high = Some(value << 4),
            }
        }
        Err(Refusal)
    }

    fn literal_string(&mut self) -> Result<Object, Refusal> {
        self.assert_next(b'(')?;
        let mut bytes = Vec::new();
        let mut depth = 1_usize;
        while let Some(byte) = self.peek() {
            self.next()?;
            match byte {
                b'\\' => self.escape(&mut bytes)?,
                b'(' => {
                    depth += 1;
                    bytes.push(byte);
                }
                b')' => {
                    depth -= 1;
                    if depth == 0 {
                        return Ok(Object::String(bytes, StringFormat::Literal));
                    }
                    bytes.push(byte);
                }
                _ => bytes.push(byte),
            }
        }
        Err(Refusal)
    }

    /// One escape sequence of a literal string, written into `bytes` the way the object model reads
    /// it: an octal code, a control character's letter, a line continuation, or the character itself.
    fn escape(&mut self, bytes: &mut Vec<u8>) -> Result<(), Refusal> {
        let Some(byte) = self.peek() else {
            return Err(Refusal);
        };
        // Three octal digits reach 0o777, one byte more than a byte holds: the low eight bits are the
        // byte, as they are for any code point a string states.
        let mut octal = 0_u8;
        let mut digits = 0;
        while digits < 3 {
            let Some(value) = self.peek().and_then(octal_value) else {
                break;
            };
            self.next()?;
            octal = octal << 3 | value;
            digits += 1;
        }
        if digits > 0 {
            bytes.push(octal);
            return Ok(());
        }
        self.next()?;
        match byte {
            b'n' => bytes.push(b'\n'),
            b'r' => bytes.push(b'\r'),
            b't' => bytes.push(b'\t'),
            b'b' => bytes.push(0x08),
            b'f' => bytes.push(0x0C),
            b'\r' => {
                if self.peek() == Some(b'\n') {
                    self.next()?;
                }
            }
            b'\n' => {}
            other => bytes.push(other),
        }
        Ok(())
    }

    /// The object a closed dictionary states: the dictionary itself, or — when a `stream` keyword
    /// follows it — a stream whose bytes are the ones the file holds.
    ///
    /// The dictionary is moved only once the walk has finished with it: every path that stops here
    /// dismantles it, since a value it states may be nested as deep as the scan builds one.
    fn dictionary_or_stream(&mut self, dictionary: Dictionary) -> Result<Object, Refusal> {
        let contents = match self.stream_after(&dictionary) {
            Ok(contents) => contents,
            Err(refusal) => {
                graph::discard(Object::Dictionary(dictionary));
                return Err(refusal);
            }
        };
        Ok(match contents {
            Some(contents) => Object::Stream(Stream::new(dictionary, contents)),
            None => Object::Dictionary(dictionary),
        })
    }

    /// The bytes a `stream` keyword after `dictionary` states, or `None` when no `stream` follows it.
    fn stream_after(&mut self, dictionary: &Dictionary) -> Result<Option<Vec<u8>>, Refusal> {
        self.skip_space_and_comments()?;
        // The keyword's own end of line is part of it, whichever spelling it takes.
        if !(self.keyword(b"stream \r\n")?
            || self.keyword(b"stream\r\n")?
            || self.keyword(b"stream\r")?
            || self.keyword(b"stream\n")?
            || self.keyword(b"stream")?)
        {
            return Ok(None);
        }
        let start = self.at;
        // The reference reads `/Length` with `dict.get`, not `lookup`, so an indirect length is not
        // resolved and falls to the end-marker scan; that is deliberate and faithful.
        let stated = dictionary
            .get(b"Length")
            .ok()
            .and_then(|length| length.as_float().ok())
            .filter(|length| *length >= 0.0)
            .map(|length| start.saturating_add(whole(length)));
        // What the recovery had to do is silent in the answer, which the reference compares, and
        // spoken in tracing, which nothing compares.
        let end = if let Some(end) = stated {
            self.seek(end.min(self.bytes.len()));
            self.skip_space_and_comments()?;
            if self.keyword(b"endstream")? {
                end.min(self.bytes.len())
            } else {
                self.seek(start);
                let found = self.end_of_stream()?;
                tracing::debug!(
                    stated = end.saturating_sub(start),
                    found = found.saturating_sub(start),
                    "document: recovered a stream whose stated length does not land on its end marker"
                );
                found
            }
        } else {
            let found = self.end_of_stream()?;
            tracing::debug!(
                found = found.saturating_sub(start),
                "document: recovered a stream that states no length"
            );
            found
        };
        Ok(Some(self.bytes[start..end].to_vec()))
    }

    /// The end of a stream's bytes, found by scanning for its own end marker: nested `stream`
    /// keywords are counted, so a file whose bytes contain one is read the way the reference reads
    /// it rather than ending early.
    ///
    /// A scan that reaches the file's end without balancing is the one expensive reading this walk
    /// does, so it is taken once: it records every token it met and the nesting each left, and a later
    /// recovery beginning inside what it read is answered from that record instead of reading the tail
    /// again.
    fn end_of_stream(&mut self) -> Result<usize, Refusal> {
        if let Some(answer) = self.recovery.as_ref().and_then(|tail| tail.answer(self.at)) {
            // A scan that found its marker stands past the keyword it read, and is asked for the
            // position before it; a refusal read nothing and stands where it was.
            if let Ok((_, after)) = &answer {
                self.seek(*after);
            }
            return answer.map(|(marker, _)| marker);
        }
        let start = self.at;
        let mut nesting = 1_i64;
        let mut tokens: Vec<Token> = Vec::new();
        // The indices whose level is still open, by increasing nesting: a token that brings the
        // nesting down closes the level above it, which is the token a later recovery inside that
        // level answers with.
        let mut open: Vec<usize> = Vec::new();
        while !self.done() {
            let begin = self.at;
            let step = if self.keyword(b"stream")? {
                1
            } else if self.keyword(b"\r\nendstream")?
                || self.keyword(b"\rendstream")?
                || self.keyword(b"\nendstream")?
                || self.keyword(b"endstream")?
            {
                -1
            } else {
                self.next()?;
                continue;
            };
            nesting += step;
            let index = tokens.len();
            while let Some(&top) = open.last()
                && tokens[top].nesting > nesting
            {
                tokens[top].closer = index;
                open.pop();
            }
            open.push(index);
            tokens.push(Token {
                start: begin,
                end: self.at,
                nesting,
                closer: usize::MAX,
            });
            if nesting == 0 {
                return Ok(begin);
            }
        }
        // The record is the tail this scan read. One that begins earlier covers everything a later one
        // could, so it is the one kept: a fresh scan that starts later — where a token straddled the
        // position a record would have answered — must not take its place.
        if self.recovery.as_ref().is_none_or(|tail| start < tail.from) {
            self.recovery = Some(Recovery {
                from: start,
                tokens,
            });
        }
        Err(Refusal)
    }

    /// The objects an object stream holds, read from its decoded bytes. The container's filter chain
    /// is [`decoded`], which reads the filters the reference reads.
    ///
    /// The container's objects are numbered by the document they join, so the container's walk shares
    /// this document's numbering and hands it back: a reference either side states is given a number
    /// the other has not given another object. A container whose `/N` or `/First` is unusable is one
    /// the reference's constructor cannot build, so it is carried as the bytes it states; what that
    /// loses is [`Lost`]'s to state.
    fn object_stream(&mut self, stream: &Stream) -> Result<(), Refusal> {
        // Both entries are read before the bytes are decoded, as the reference's constructor reads
        // them: a container either of them cannot build is one it never decodes.
        let count = self.container_count(stream);
        if let (Some(count), Some(first)) = (count, self.container_first(stream))
            && let Some(data) = decoded(stream, &self.objects, self.bounds.decode)
            // What the container's bytes could hold: one entry per four bytes, the least two numbers
            // and the space between them take. A `/N` past that names entries the bytes do not state,
            // which is where the reference's own parser runs out of them.
            && count <= data.len() / 4
        {
            return self.expand(&data, count, first);
        }
        // Nothing can be read from it: no `/N` or `/First` its constructor accepts, no decode, or a
        // count its bytes could not hold. The file states a count and the produced file holds none of
        // them.
        if let Some(count) = count {
            self.numbering.lost.add_unread(count);
        }
        Err(Refusal)
    }

    /// The objects an object stream's decoded bytes hold, placed as they are read, and the ones it
    /// states and does not place reported. A container whose later entry does not parse keeps the
    /// objects read before that one, as the reference keeps them: both sides assign each object to the
    /// document as they parse it.
    fn expand(&mut self, data: &[u8], count: usize, first: i64) -> Result<(), Refusal> {
        // The container's bytes are bounded by their own decoded length, not by the file's: a modern
        // file compresses far better than any share of the file's bytes would allow for.
        let mut nested = Scan::new(data, self.bounds);
        // The objects the container holds are numbered by the document they join, so the numbering is
        // shared with the container's walk and handed back when it is done.
        nested.share_numbering(self);
        // An entry states no generation, so the second run of every entry is empty — the same run for
        // all of them, and not the one a file's own header states.
        let entry = |at: usize| Runs {
            number: at,
            generation: None,
        };
        let mut entries: Vec<(Key, Runs, u32)> = Vec::new();
        let mut joined: Vec<(Key, Runs, Object)> = Vec::new();
        let outcome = (|| {
            for _ in 0..count {
                nested.skip_space_and_comments()?;
                let runs = entry(nested.at);
                let number = nested.raw_int()?;
                let id = nested.stated(number, 0, runs);
                nested.skip_space_and_comments()?;
                let offset = u32::try_from(nested.raw_int()?).map_err(|_| Refusal)?;
                entries.push((id, runs, offset));
            }
            for &(id, runs, offset) in &entries {
                // An entry is read at `/First` plus its own offset, as the reference's stream places it.
                // A position before the container's bytes reads nothing there, exactly as one past their
                // end does, and a position this host cannot hold is one past them.
                let position = first.saturating_add(i64::from(offset));
                let at = usize::try_from(position).unwrap_or(nested.bytes.len());
                nested.seek(at.min(nested.bytes.len()));
                joined.push((id, runs, nested.object(nested.bounds.depth)?));
            }
            tracing::debug!(objects = count, "document: expanded an object stream");
            Ok(())
        })();
        nested.share_numbering(self);
        let placed = joined.len();
        for (id, runs, object) in joined {
            self.assign(id, data, runs, object);
        }
        self.numbering.lost.objects = self
            .numbering
            .lost
            .objects
            .saturating_add(count.saturating_sub(placed));
        outcome
    }

    /// A container's `/N`, read the way the reference's constructor reads it: the value at one step,
    /// and a number. `None` when the dictionary states no `/N`, states one that names nothing, or
    /// states something that is not a number — a container its constructor throws on.
    ///
    /// The reference counts `for (idx = 0; idx < count; idx++)`, so a negative count reads no entry
    /// and a fractional one reads the entries below the next whole number above it: `/N 1.5` reads
    /// two and `/N 0.5` reads one. A count written as an integer is read exactly; one written as a
    /// real is read at the value the object model holds for a real, which is single precision, so a
    /// count above 2^24 written that way reads at that value (`PARSER.md` §3.5).
    fn container_count(&self, stream: &Stream) -> Option<usize> {
        Some(match self.container_number(stream, b"N")? {
            // A count past what this host can hold names more entries than any file's bytes could
            // hold: it is kept at the host's ceiling, which the capacity check refuses, so the
            // container is carried and reported rather than expanded to nothing.
            Numeric::Whole(count) => match usize::try_from(count) {
                Ok(count) => count,
                Err(_) if count < 0 => 0,
                Err(_) => usize::MAX,
            },
            Numeric::Real(count) => whole(count.ceil()),
        })
    }

    /// A container's `/First`, read the same way: the offset an entry's own offset is added to. A whole
    /// number names a position whatever its sign — [`Scan::expand`] reads an entry at the sum — while a
    /// fraction names none a byte stream can index, so a container stating one is carried.
    fn container_first(&self, stream: &Stream) -> Option<i64> {
        match self.container_number(stream, b"First")? {
            Numeric::Whole(first) => Some(first),
            Numeric::Real(first) if whole_number(first) => Some(signed_whole(first)),
            Numeric::Real(_) => None,
        }
    }

    /// The number a container's dictionary states at `key`, one step through the object map, or
    /// nothing where it states no number at all — the typed lookup its constructor does.
    fn container_number(&self, stream: &Stream, key: &[u8]) -> Option<Numeric> {
        match graph::resolved(&self.objects, stream.dict.get(key).ok()?) {
            Object::Integer(value) => Some(Numeric::Whole(*value)),
            Object::Real(value) => Some(Numeric::Real(*value)),
            _ => None,
        }
    }

    /// An index stream's dictionary states the trailer entries, and the stream is never kept as an
    /// object. Entries are adopted only when the stream decodes and `/Size`, `/W` and `/Index` resolve
    /// the way the reference's constructor requires: `/Size` a number, `/W` an array whose every
    /// element is a number, and `/Index` — when the dictionary states it as an array — an even number
    /// of numeric elements. Any failure adopts nothing, which is what the reference's constructor does
    /// by throwing before it states anything. The table itself is not read: the reference reads it into
    /// entries it never uses, and past the end of the bytes it reads zeros rather than refusing, so a
    /// truncated table changes nothing on either side. Unlike a `trailer` dictionary, an index stream
    /// *replaces* what an earlier statement said, so an entry it leaves out is no longer stated at all.
    ///
    /// The decoded bytes are held once and dropped, whichever filter states them. A container the
    /// engine cannot decode — a filter chain it does not read, or one whose bytes pass
    /// `container_decode_max_bytes` — adopts nothing, which is what the reference's constructor does by
    /// throwing. `false` is a dictionary it throws on, and a stream whose index it cannot read is
    /// carried as the bytes it states rather than dropped, which is what the reference does with the
    /// object it could not parse.
    fn index_stream(&mut self, stream: &Stream) -> bool {
        let dictionary = &stream.dict;
        let Ok(size) = dictionary.get(b"Size") else {
            return false;
        };
        if graph::resolved(&self.objects, size).as_float().is_err() {
            return false;
        }
        let Ok(widths) = dictionary.get(b"W") else {
            return false;
        };
        let Ok(widths) = graph::resolved(&self.objects, widths).as_array() else {
            return false;
        };
        if !widths
            .iter()
            .all(|width| graph::resolved(&self.objects, width).as_float().is_ok())
        {
            return false;
        }
        // The reference falls back to `[0 Size]` when `/Index` is not an array, and throws only on an
        // element that is not a number, so a `/Index` of another shape is no statement at all.
        if let Ok(index) = dictionary.get(b"Index")
            && let Ok(index) = graph::resolved(&self.objects, index).as_array()
            && (index.len() % 2 != 0
                || !index
                    .iter()
                    .all(|value| graph::resolved(&self.objects, value).as_float().is_ok()))
        {
            return false;
        }
        if decoded(stream, &self.objects, self.bounds.decode).is_none() {
            return false;
        }
        self.replace_trailer(dictionary);
        true
    }
}

/// The entries a `trailer` dictionary or an index stream's dictionary states.
const TRAILER_KEYS: &[&[u8]] = &[b"Root", b"Encrypt", b"Info", b"ID"];

/// Whether `object` is a catalog: a dictionary that states `/Type /Catalog` itself, the test the
/// reference builds a `PDFCatalog` by — `parseDict` compares the entry it read, not what it names, so
/// `/Type` stated as a reference is no catalog. The object itself is followed through a reference,
/// because root recovery reads `/Root` with a lookup. `page::catalog` applies the same test at load.
fn is_catalog(object: &Object, objects: &BTreeMap<ObjectId, Object>) -> bool {
    match graph::resolved(objects, object) {
        Object::Dictionary(dictionary) => dictionary
            .get(b"Type")
            .is_ok_and(|kind| kind.as_name().is_ok_and(|name| name == b"Catalog")),
        _ => false,
    }
}

/// A dictionary's `/Type`, resolved through the object map — which is how the reference decides a
/// container's kind (`dict.lookup`) and *not* how it decides a catalog's (`dict.get`, see
/// [`is_catalog`]).
fn resolved_type<'a>(
    dictionary: &'a Dictionary,
    objects: &'a BTreeMap<ObjectId, Object>,
) -> Option<&'a [u8]> {
    let value = dictionary.get(b"Type").ok()?;
    graph::resolved(objects, value).as_name().ok()
}

fn is_space(byte: u8) -> bool {
    matches!(byte, 0 | b'\t' | b'\n' | 0x0C | b'\r' | b' ')
}

fn is_delimiter(byte: u8) -> bool {
    matches!(
        byte,
        b'(' | b')' | b'<' | b'>' | b'[' | b']' | b'{' | b'}' | b'/' | b'%'
    )
}

fn is_digit(byte: u8) -> bool {
    byte.is_ascii_digit()
}

/// A number with `byte`'s digit appended, saturating at the model's own ceiling: a run of digits
/// longer than a `u64` is the largest number rather than a failure.
fn shift(value: u64, byte: u8) -> u64 {
    value
        .saturating_mul(10)
        .saturating_add(u64::from(byte - b'0'))
}

/// The run of digits a number starts with at `at`: [`Scan::raw_int`] accepts no sign, so every place
/// the walk takes a run from is the first digit of one.
fn digits_at(bytes: &[u8], at: usize) -> &[u8] {
    let rest = bytes.get(at..).unwrap_or_default();
    let end = rest
        .iter()
        .position(|byte| !is_digit(*byte))
        .unwrap_or(rest.len());
    &rest[..end]
}

/// The bits of the double the reference reads a run of digits as: `parseRawInt` converts the run
/// with `Number`, so an object number is a double there, and two runs that round to the same double
/// are one number to it while two that do not are two. `number` is what this model made of the same
/// run, and it is the answer only when `digits` says nothing a double holds: an empty slice for a
/// generation a caller states none of, which only a container's entry table does.
fn double(digits: &[u8], number: u64) -> u64 {
    // The precision loss is the reading: a number this model saturated is one the reference holds as a
    // double with its own rounded value, so the whole number is what it reads.
    #[expect(
        clippy::cast_precision_loss,
        reason = "a run of digits the model saturated is one the reference holds as a double with its \
                  own rounded value, so the rounded whole number is what it reads"
    )]
    let whole = number as f64;
    std::str::from_utf8(digits)
        .ok()
        .and_then(|digits| digits.parse::<f64>().ok())
        .unwrap_or(whole)
        .to_bits()
}

fn is_numeric(byte: u8) -> bool {
    byte.is_ascii_digit() || matches!(byte, b'+' | b'-' | b'.')
}

/// Whether a real names a whole number: its truncation states the same bits, which is a reading of the
/// value rather than a float comparison's tolerance.
fn whole_number(value: f32) -> bool {
    value.trunc().to_bits() == value.to_bits()
}

/// A real that names a whole number, as that number: both signs, because a container's `/First` is a
/// position the entry's own offset is added to, and one before the container's bytes reads nothing.
#[expect(
    clippy::cast_possible_truncation,
    reason = "a real whose truncation states the same bits is a whole number, and one past the `i64` \
              bounds saturates to a position past any container's bytes"
)]
fn signed_whole(value: f32) -> i64 {
    value as i64
}

/// The bytes of every body the walk carried, from the objects `objects` states.
fn carried_bodies<'a>(objects: impl Iterator<Item = &'a Object>) -> impl Iterator<Item = &'a [u8]> {
    objects.filter_map(graph::carried)
}

/// One reference the bytes of a body the walk carried state: the number that body writes, read the two
/// ways this walk reads one — saturated into the model's own `u64`, which is the number a reference may
/// keep free, and as the double the reference keys an object number by, which is the number an object
/// is placed under — with the generation keyed the same way.
#[derive(Clone, Copy, Debug)]
struct Stated {
    number: u64,
    key: u64,
    key_generation: u64,
}

/// Every reference the bytes of an object the walk carried state. Such a body is not parsed — that is
/// what carrying it means — so this is the only way to know which numbers its references name: a
/// number one of them states must stay free, and one whose object is given a number of this
/// document's own is a reference the produced file no longer answers.
///
/// It steps over what cannot state a reference — a comment to the end of its line, a literal or
/// hexadecimal string, a name — so a number inside one is not mistaken for one. What no byte scan can
/// tell is a reference from three numbers that happen to lie together in bytes the file never meant
/// as an object; a body this damaged is read by a keyword scan, not by a parser, and that is the
/// reading the reference's own recovery does too.
fn carried_references(bytes: &[u8], found: &mut impl FnMut(Stated)) {
    let run = |at: &mut usize| {
        let mut value = 0_u64;
        while let Some(&byte) = bytes.get(*at) {
            if !is_digit(byte) {
                break;
            }
            value = shift(value, byte);
            *at += 1;
        }
        value
    };
    let space = |at: &mut usize| {
        while bytes.get(*at).is_some_and(|byte| is_space(*byte)) {
            *at += 1;
        }
    };
    let mut at = 0;
    while let Some(&byte) = bytes.get(at) {
        match byte {
            b'%' => {
                while bytes
                    .get(at)
                    .is_some_and(|byte| *byte != b'\n' && *byte != b'\r')
                {
                    at += 1;
                }
            }
            b'(' => at = past_literal(bytes, at),
            b'<' if bytes.get(at + 1) == Some(&b'<') => at += 2,
            b'<' => {
                while bytes.get(at).is_some_and(|byte| *byte != b'>') {
                    at += 1;
                }
                at += 1;
            }
            b'/' => {
                at += 1;
                while bytes
                    .get(at)
                    .is_some_and(|byte| !is_space(*byte) && !is_delimiter(*byte))
                {
                    at += 1;
                }
            }
            byte if is_digit(byte) => {
                let start = at;
                let number = run(&mut at);
                let ran = at;
                let first = at;
                space(&mut at);
                if at == first || !bytes.get(at).is_some_and(|byte| is_digit(*byte)) {
                    at = first;
                    continue;
                }
                let generation_at = at;
                let generation = run(&mut at);
                let built = at;
                let second = at;
                space(&mut at);
                // `R` has to follow the generation with whitespace between: `1 0R` is not a reference.
                if at != second
                    && bytes.get(at) == Some(&b'R')
                    && bytes
                        .get(at + 1)
                        .is_none_or(|byte| is_space(*byte) || is_delimiter(*byte))
                {
                    found(Stated {
                        number,
                        key: double(&bytes[start..ran], number),
                        key_generation: double(&bytes[generation_at..built], generation),
                    });
                }
                at = second;
            }
            _ => at += 1,
        }
    }
}

/// The position past the literal string beginning at `at`: nested parentheses count, and a backslash
/// escapes the byte after it.
fn past_literal(bytes: &[u8], mut at: usize) -> usize {
    let mut depth = 0_usize;
    while let Some(&byte) = bytes.get(at) {
        at += 1;
        match byte {
            b'\\' => at += 1,
            b'(' => depth += 1,
            b')' => {
                depth -= 1;
                if depth == 0 {
                    break;
                }
            }
            _ => {}
        }
    }
    at.min(bytes.len())
}

fn hex_value(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

fn octal_value(byte: u8) -> Option<u8> {
    (b'0'..=b'7').contains(&byte).then(|| byte - b'0')
}

/// A name's bytes with the `#xx` escapes the reference decodes: a `#` followed by two upper-case
/// hexadecimal digits is the byte they state, and any other `#` is itself. A name that states no
/// escape is handed back as it was read, which is the case for nearly every name in a file.
fn decode_name(name: Vec<u8>) -> Vec<u8> {
    if !name.contains(&b'#') {
        return name;
    }
    let escape = |at: usize| {
        let pair = name.get(at + 1..at + 3)?;
        let (high, low) = (hex_value(pair[0])?, hex_value(pair[1])?);
        (!pair.iter().any(u8::is_ascii_lowercase)).then_some(high << 4 | low)
    };
    let mut decoded = Vec::with_capacity(name.len());
    let mut index = 0;
    while index < name.len() {
        if name[index] == b'#'
            && let Some(byte) = escape(index)
        {
            decoded.push(byte);
            index += 3;
            continue;
        }
        decoded.push(name[index]);
        index += 1;
    }
    decoded
}

/// The stream's bytes through its filter chain, or `None` for a chain this engine does not decode.
/// `FlateDecode`, `LZWDecode`, `ASCIIHexDecode` and `RunLengthDecode` are decoded here, the way the
/// reference's library decodes them; every other filter is handed to the object model one stage at a
/// time.
///
/// `/Filter` and `/DecodeParms` are resolved through the object map, as the reference resolves them,
/// so a chain stated indirectly is the chain it names. Each stage's output is the next stage's input,
/// so the cost is the chain's own product rather than the file's bytes. `max` is the ceiling: of the
/// stages decoded here, one that expands consults it while it produces its bytes — [`lzw::decode`]
/// caps its own reservation at it — and one past it is abandoned rather than built, so the container
/// is carried rather than the file refused. A stage that only shrinks, or one the object model
/// decodes, is measured once it exists.
fn decoded(stream: &Stream, objects: &BTreeMap<ObjectId, Object>, max: u64) -> Option<Vec<u8>> {
    let filters = filters(stream, objects)?;
    chain(stream, &filters, objects, max)
}

/// `filters` applied in order. Each stage's parameters are borrowed from the document rather than
/// copied, because a `/DecodeParms` value may be nested as deep as the scan builds one; only the
/// filters the object model decodes need a copy of their own.
fn chain(
    stream: &Stream,
    filters: &[(Vec<u8>, Option<&Dictionary>)],
    objects: &BTreeMap<ObjectId, Object>,
    max: u64,
) -> Option<Vec<u8>> {
    let mut data = stream.content.clone();
    for (filter, parms) in filters {
        data = match filter.as_slice() {
            b"ASCIIHexDecode" => ascii_hex(&data),
            b"RunLengthDecode" => run_length(&data, max)?,
            b"FlateDecode" => inflate(&data, max)?,
            b"LZWDecode" => lzw::decode(&data, early_change(*parms, objects), max)?,
            _ => {
                // One filter at a time, over the bytes the one before left: the dictionary is copied
                // for the single-filter stream and the content is not.
                let mut single = Stream::new(graph::clone_dictionary(&stream.dict), data);
                graph::set(&mut single.dict, b"Filter", Object::Name(filter.clone()));
                match parms {
                    Some(parms) => {
                        graph::set(
                            &mut single.dict,
                            b"DecodeParms",
                            Object::Dictionary(graph::clone_dictionary(parms)),
                        );
                    }
                    None => {
                        if let Some(removed) = single.dict.remove(b"DecodeParms") {
                            graph::discard(removed);
                        }
                    }
                }
                let decoded = single.decompressed_content().ok();
                // The dictionary the single-filter stream states was copied for it, so it is taken
                // apart here rather than by the language's own drop.
                graph::discard(Object::Stream(single));
                decoded?
            }
        };
        if data.len() as u64 > max {
            return None;
        }
    }
    Some(data)
}

/// A stream's filter chain, each filter with the decode parameters stated for it. `/Filter` and
/// `/DecodeParms` are resolved through the object map, because the reference reads them with
/// `lookup`: a `/Filter` that names an object is the name, or the array of names, it names.
///
/// `None` is a chain this engine does not read: a `/Filter` that resolves to neither a name nor an
/// array of names, or an array whose element is not a name.
fn filters<'a>(
    stream: &'a Stream,
    objects: &'a BTreeMap<ObjectId, Object>,
) -> Option<Vec<(Vec<u8>, Option<&'a Dictionary>)>> {
    let Ok(filter) = stream.dict.get(b"Filter") else {
        return Some(Vec::new());
    };
    let parms = stream
        .dict
        .get(b"DecodeParms")
        .ok()
        .map(|parms| graph::resolved(objects, parms));
    match graph::resolved(objects, filter) {
        // A chain of one reads the parameters as they stand, so an array states none.
        Object::Name(name) => {
            let parms = parms.and_then(|parms| parms.as_dict().ok());
            Some(vec![(name.clone(), parms)])
        }
        // A chain of several reads each filter's own entry, so a single dictionary states none.
        Object::Array(names) => {
            let mut chain: Vec<(Vec<u8>, Option<&Dictionary>)> = Vec::with_capacity(names.len());
            for (index, name) in names.iter().enumerate() {
                let name = graph::resolved(objects, name)
                    .as_name()
                    .ok()
                    .map(<[u8]>::to_vec)?;
                let parms = match parms {
                    Some(Object::Array(values)) => values.get(index),
                    _ => None,
                }
                .and_then(|parms| graph::resolved(objects, parms).as_dict().ok());
                chain.push((name, parms));
            }
            Some(chain)
        }
        _ => None,
    }
}

/// `/EarlyChange` as the reference reads it: the number the entry states, resolved through the object
/// map, and the format's default of one when the entry is absent or is not a number. A whole number is
/// put in as the double the reference's parser holds for it, and a real at the object model's single
/// precision, which is `PARSER.md` §6.3's model; where that rounding moves a width crossing, §3.15
/// states the divergence.
#[expect(
    clippy::cast_precision_loss,
    reason = "a whole number past two to the fifty-third is read at the value a double holds for it, \
              which is the value the reference's own parser holds"
)]
fn early_change(parms: Option<&Dictionary>, objects: &BTreeMap<ObjectId, Object>) -> f64 {
    let Some(stated) = parms
        .and_then(|parms| parms.get(b"EarlyChange").ok())
        .map(|value| graph::resolved(objects, value))
    else {
        return 1.0;
    };
    match stated {
        Object::Integer(whole) => *whole as f64,
        Object::Real(fraction) => f64::from(*fraction),
        _ => 1.0,
    }
}

/// ASCIIHexDecode: hexadecimal digits, whitespace ignored, and `>` ends the data. An odd final digit is
/// padded with a zero only when `>` ended the data — the reference pads it when that marker set its end
/// of file and drops it otherwise — so a stream that simply runs out loses its trailing nibble.
fn ascii_hex(data: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut high: Option<u8> = None;
    let mut ended = false;
    for &byte in data {
        if byte == b'>' {
            ended = true;
            break;
        }
        let Some(value) = hex_value(byte) else {
            continue;
        };
        match high.take() {
            Some(first) => out.push(first << 4 | value),
            None => high = Some(value),
        }
    }
    if let Some(first) = high.filter(|_| ended) {
        out.push(first << 4);
    }
    out
}

/// FlateDecode as the reference's own decoder reads it: a zlib header the format allows, then deflate
/// blocks that must reach their last one. A stream whose last block is cut short is not one the
/// reference decodes — its decoder throws and the container is kept as the bytes it states — and
/// neither side reads the checksum that follows the last block, so a stream missing only that decodes
/// whole. `/DecodeParms` is not applied, because the reference's decoder does not read it for this
/// filter. `None` is a stream that is not one this decodes, or one past `max`.
fn inflate(data: &[u8], max: u64) -> Option<Vec<u8>> {
    let [cmf, flg, deflate @ ..] = data else {
        return None;
    };
    // One compression method, a header whose check byte matches, and no preset dictionary: the three
    // things the reference's decoder refuses a stream for before it reads a block.
    if cmf & 0x0F != 8 || (u16::from(*cmf) << 8 | u16::from(*flg)) % 31 != 0 || flg & 0x20 != 0 {
        return None;
    }
    let mut decompress = Decompress::new(false);
    let mut out = Vec::new();
    let mut chunk = [0_u8; 8192];
    let mut input = deflate;
    loop {
        let before_in = decompress.total_in();
        let before_out = decompress.total_out();
        let status = decompress
            .decompress(input, &mut chunk, FlushDecompress::None)
            .ok()?;
        let consumed = usize::try_from(decompress.total_in() - before_in).ok()?;
        let produced = usize::try_from(decompress.total_out() - before_out).ok()?;
        if out.len() as u64 + produced as u64 > max {
            return None;
        }
        out.extend_from_slice(&chunk[..produced]);
        input = &input[consumed..];
        match status {
            Status::StreamEnd => return Some(out),
            // No input taken and no output made: the bytes ran out before the last block ended.
            _ if consumed == 0 && produced == 0 => return None,
            _ => {}
        }
    }
}

/// RunLengthDecode: a length byte `n`; `0..=127` copies the next `n + 1` bytes, `129..=255` repeats
/// the next byte `257 - n` times, and `0x80` ends the data. Bytes running out end the data there, which
/// is what the reference's own decoder does: it reads the two bytes a run's header occupies and takes
/// the bytes the stream still holds, so a run the bytes do not fill is as long as they are and no
/// longer. Each run is measured before it is written, so no stage can hold more than `max`.
fn run_length(data: &[u8], max: u64) -> Option<Vec<u8>> {
    let mut out = Vec::new();
    let mut index = 0;
    loop {
        let Some(&header) = data.get(index) else {
            return Some(out);
        };
        index += 1;
        let (count, byte) = match header {
            0x80 => return Some(out),
            0..=127 => {
                let count = usize::from(header) + 1;
                if out.len() as u64 + count as u64 > max {
                    return None;
                }
                let held = data.len().saturating_sub(index).min(count);
                out.extend_from_slice(&data[index..index + held]);
                index += held;
                if held < count {
                    return Some(out);
                }
                continue;
            }
            _ => {
                let Some(&byte) = data.get(index) else {
                    return Some(out);
                };
                (257 - usize::from(header), byte)
            }
        };
        index += 1;
        if out.len() as u64 + count as u64 > max {
            return None;
        }
        out.resize(out.len() + count, byte);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The statement a test's header makes of a number the model holds: the run of digits as the bytes
    /// state it, which is where [`Scan::assign`] reads the values a redefinition is told by.
    fn held(number: &str) -> (&[u8], Runs) {
        (
            number.as_bytes(),
            Runs {
                number: 0,
                generation: None,
            },
        )
    }

    /// An object stream holding two integers, `4 0 R` = 11 and `5 0 R` = 22.
    fn container() -> Stream {
        let header = "4 0 5 3 ";
        let mut data = header.as_bytes().to_vec();
        data.extend_from_slice(b"11 22");
        let mut dict = Dictionary::new();
        dict.set("Type", Object::Name(b"ObjStm".to_vec()));
        dict.set("N", Object::Integer(2));
        dict.set("First", Object::Integer(8));
        Stream::new(dict, data)
    }

    #[test]
    fn a_file_with_bytes_before_its_header_is_read() {
        // `parseHeader` scans forward for `%PDF-`, so a file with a mail header, a byte-order mark or a
        // partial line before the marker is still a file.
        let file = b"From: someone\nSubject: a file\n%PDF-1.7\n\
                     1 0 obj\n<< /Type /Catalog >>\nendobj\n\
                     trailer\n<< /Root 1 0 R >>\n%%EOF\n";
        let (document, _) = super::document(file, Bounds::stated()).expect("the file reads");
        assert_eq!(document.version, "1.7");
        assert!(document.objects.contains_key(&(1, 0)), "the body was read");
    }

    #[test]
    fn a_stray_line_between_two_objects_is_skipped() {
        // `skipJibberish` steps over anything that is not an object header, an `xref`, a `trailer` or a
        // `startxref`, so a body with a stray line between two objects reads.
        let file = b"%PDF-1.7\n\
                     1 0 obj\n<< /Type /Catalog >>\nendobj\n\
                     this line is not a token any reader expects\n\
                     2 0 obj\nnull\nendobj\n\
                     trailer\n<< /Root 1 0 R >>\n%%EOF\n";
        let (document, _) = super::document(file, Bounds::stated()).expect("the file reads");
        assert!(
            document.objects.contains_key(&(2, 0)),
            "the second object was read"
        );
    }

    #[test]
    fn an_object_without_an_endobj_is_read() {
        // `parseIndirectObject` matches `endobj` and ignores whether it found one, so a file without
        // the marker answers as the same file with it does.
        let file = b"%PDF-1.7\n\
                     1 0 obj\n<< /Type /Catalog >>\n\
                     2 0 obj\nnull\nendobj\n\
                     trailer\n<< /Root 1 0 R >>\n%%EOF\n";
        let (document, _) = super::document(file, Bounds::stated()).expect("the file reads");
        assert!(
            document.objects.contains_key(&(1, 0)),
            "the object was read"
        );
        assert!(
            document.objects.contains_key(&(2, 0)),
            "the walk carried on"
        );
    }

    #[test]
    fn an_object_at_the_reserved_number_is_dropped() {
        // `parseDocument` deletes object `0 0` after the walk because nothing can reference it.
        let file = b"%PDF-1.7\n\
                     0 0 obj\nnull\nendobj\n\
                     1 0 obj\n<< /Type /Catalog >>\nendobj\n\
                     trailer\n<< /Root 1 0 R >>\n%%EOF\n";
        let (document, lost) = super::document(file, Bounds::stated()).expect("the file reads");
        assert!(
            !document.objects.contains_key(&(0, 0)),
            "the reserved object is gone"
        );
        assert_eq!(lost.objects, 0, "nothing a reader can see was lost");
    }

    #[test]
    fn a_binary_mark_after_the_header_is_stepped_over() {
        // `skipBinaryHeaderComment` steps one byte past whatever follows the version line unless it is
        // an object header, so the four-byte mark a binary file states is not read as an object.
        let file = b"%PDF-1.7\n%\xE2\xE3\xCF\xD3\n\
                     1 0 obj\n<< /Type /Catalog >>\nendobj\n\
                     trailer\n<< /Root 1 0 R >>\n%%EOF\n";
        let (document, _) = super::document(file, Bounds::stated()).expect("the file reads");
        assert_eq!(document.version, "1.7");
        assert_eq!(document.objects.len(), 1, "the mark is not an object");
    }

    #[test]
    fn a_comment_between_two_tokens_is_skipped() {
        // `skipWhitespaceAndComments` skips `%` to the end of the line wherever it meets one, so a
        // comment inside an object header changes nothing.
        let file = b"%PDF-1.7\n\
                     1 % the number, then the generation\n0 obj\n<< /Type /Catalog >>\nendobj\n\
                     trailer\n<< /Root 1 0 R >>\n%%EOF\n";
        let (document, _) = super::document(file, Bounds::stated()).expect("the file reads");
        assert!(
            document.objects.contains_key(&(1, 0)),
            "the header was read"
        );
    }

    #[test]
    fn an_unbalanced_string_is_carried_as_its_own_bytes() {
        // `parseString` throws on an unbalanced parenthesis, so the object is kept as the bytes it
        // states rather than refused.
        let file = b"%PDF-1.7\n\
                     1 0 obj\n<< /Type /Catalog >>\nendobj\n\
                     2 0 obj\n<< /Title (unbalanced >>\nendobj\n\
                     trailer\n<< /Root 1 0 R >>\n%%EOF\n";
        let (document, _) = super::document(file, Bounds::stated()).expect("the file reads");
        assert!(
            document
                .objects
                .get(&(2, 0))
                .and_then(graph::carried)
                .is_some(),
            "it is not a parsed object"
        );
    }

    #[test]
    fn a_stream_without_an_end_marker_is_carried_as_its_own_bytes() {
        // `findEndOfStreamFallback` throws when the nesting never returns to zero, so the object is
        // carried as the bytes it states.
        let file = b"%PDF-1.7\n\
                     1 0 obj\n<< /Type /Catalog >>\nendobj\n\
                     2 0 obj\n<< /Length 4 >>\nstream\nabcd\nendobj\n\
                     trailer\n<< /Root 1 0 R >>\n%%EOF\n";
        let (document, _) = super::document(file, Bounds::stated()).expect("the file reads");
        assert!(
            document
                .objects
                .get(&(2, 0))
                .and_then(graph::carried)
                .is_some(),
            "the object is carried as its own bytes"
        );
    }

    #[test]
    fn a_number_may_start_with_a_sign_or_a_point() {
        // `parseRawNumber` accepts `+`, `-` and `.` as the first byte, so a number stated that way is
        // the number it spells.
        for (stated, expected) in [("+5", 5.0), ("-5", -5.0), (".5", 0.5)] {
            let file = format!(
                "%PDF-1.7\n1 0 obj\n<< /Type /Catalog /Value {stated} >>\nendobj\n\
                 trailer\n<< /Root 1 0 R >>\n%%EOF\n"
            );
            let (document, _) =
                super::document(file.as_bytes(), Bounds::stated()).expect("the file reads");
            let value = document
                .objects
                .get(&(1, 0))
                .and_then(|object| object.as_dict().ok())
                .and_then(|dictionary| dictionary.get(b"Value").ok())
                .and_then(|value| value.as_float().ok())
                .expect("the value was read");
            assert!(
                (f64::from(value) - expected).abs() < f64::EPSILON,
                "{stated} was read as {value}"
            );
        }
    }

    #[test]
    fn an_object_stream_is_bounded_by_its_own_bytes() {
        // A container holds far more than the file that states it, so the nested walk gets its own
        // allowance: a parent with nothing left still reads the objects the container holds.
        let mut scan = Scan::new(b"", Bounds::stated());
        scan.object_stream(&container())
            .expect("the container is read");
        assert_eq!(scan.objects.len(), 2);
    }

    #[test]
    fn an_object_deeper_than_the_scan_builds_is_not_read() {
        // The parser stops at the bound and answers the same refusal a body it cannot parse gives; what
        // the walk then does with the object is the caller's, and
        // [`an_object_deeper_than_the_scan_builds_is_carried_as_its_own_bytes`] pins that it is carried.
        let mut shallow = Scan::new(
            b"[[[[1]]]]",
            Bounds {
                depth: 4,
                ..Bounds::stated()
            },
        );
        assert!(shallow.object(shallow.bounds.depth).is_ok());
        let mut deep = Scan::new(
            b"[[[[[1]]]]]",
            Bounds {
                depth: 4,
                ..Bounds::stated()
            },
        );
        assert!(matches!(deep.object(deep.bounds.depth), Err(Refusal)));
    }

    #[test]
    fn a_dictionary_whose_last_key_states_no_value_is_not_read() {
        // The reference's own parser throws here and keeps the object's bytes, so this walk stops
        // rather than repairing the dictionary into one the file does not state.
        let mut scan = Scan::new(b"<< /Type /Font /BaseFont >>", Bounds::stated());
        assert!(matches!(scan.object(scan.bounds.depth), Err(Refusal)));
        // A key that states a value before the next key is read is a dictionary as stated.
        let mut scan = Scan::new(b"<< /Type /Font /BaseFont /Helvetica >>", Bounds::stated());
        assert!(scan.object(scan.bounds.depth).is_ok());
    }

    #[test]
    fn an_index_stream_whose_bytes_cannot_be_decoded_is_not_adopted() {
        // The reference decodes the stream before it reads its dictionary, so a container it cannot
        // decode is an object of no kind and its trailer entries are never taken.
        let mut scan = Scan::new(b"", Bounds::stated());
        let mut dict = Dictionary::new();
        dict.set("Type", Object::Name(b"XRef".to_vec()));
        dict.set("Size", Object::Integer(1));
        dict.set("W", Object::Array(vec![Object::Integer(1)]));
        dict.set("Root", Object::Reference((1, 0)));
        dict.set("Encrypt", Object::Reference((2, 0)));
        dict.set("Filter", Object::Name(b"JPXDecode".to_vec()));
        let stream = Stream::new(dict, b"not a compressed stream".to_vec());
        scan.index_stream(&stream);
        assert!(scan.trailer.get(b"Root").is_err());
        assert!(scan.trailer.get(b"Encrypt").is_err());
    }

    #[test]
    fn an_index_stream_whose_numbers_do_not_resolve_is_not_adopted() {
        // The reference's constructor resolves `/Size`, `/W` and `/Index` and throws on a value of
        // the wrong shape, so such a container states no trailer entries at all.
        for mut dict in [
            {
                let mut dict = Dictionary::new();
                dict.set("Size", Object::Name(b"Size".to_vec()));
                dict.set("W", Object::Array(vec![Object::Integer(1)]));
                dict
            },
            {
                let mut dict = Dictionary::new();
                dict.set("Size", Object::Integer(1));
                dict.set("W", Object::Array(vec![Object::Name(b"W".to_vec())]));
                dict
            },
            {
                let mut dict = Dictionary::new();
                dict.set("Size", Object::Integer(1));
                dict.set("W", Object::Array(vec![Object::Integer(1)]));
                dict.set("Index", Object::Array(vec![Object::Integer(1)]));
                dict
            },
        ] {
            let mut scan = Scan::new(b"", Bounds::stated());
            dict.set("Root", Object::Reference((1, 0)));
            scan.index_stream(&Stream::new(dict, Vec::new()));
            assert!(scan.trailer.get(b"Root").is_err());
        }
    }

    #[test]
    fn an_object_stream_whose_filter_is_ascii_hex_is_expanded() {
        // The reference decodes ASCIIHex, which lopdf does not; a container filtered with it is
        // still a container.
        let mut data = b"4 0 5 3 ".to_vec();
        data.extend_from_slice(b"11 22");
        let mut hex = Vec::new();
        for byte in data {
            hex.extend_from_slice(format!("{byte:02X}").as_bytes());
        }
        let mut dict = Dictionary::new();
        dict.set("Type", Object::Name(b"ObjStm".to_vec()));
        dict.set("N", Object::Integer(2));
        dict.set("First", Object::Integer(8));
        dict.set("Filter", Object::Name(b"ASCIIHexDecode".to_vec()));
        let mut scan = Scan::new(b"", Bounds::stated());
        scan.object_stream(&Stream::new(dict, hex))
            .expect("the container is read");
        assert_eq!(scan.objects.get(&(4, 0)), Some(&Object::Integer(11)));
        assert_eq!(scan.objects.get(&(5, 0)), Some(&Object::Integer(22)));
    }

    #[test]
    fn a_run_length_encoded_stream_is_decoded() {
        let mut dict = Dictionary::new();
        dict.set("Filter", Object::Name(b"RunLengthDecode".to_vec()));
        let stream = Stream::new(dict, vec![2, b'a', b'b', b'c', 254, b'z', 128]);
        assert_eq!(
            decoded(&stream, &BTreeMap::new(), Bounds::stated().decode)
                .expect("the stream decodes"),
            b"abczzz"
        );
    }

    #[test]
    fn a_truncated_run_length_stream_decodes_the_bytes_it_states() {
        // The reference reads the bytes the stream still holds and no more, so a run the bytes do not
        // fill is as long as they are: a run of six that states one byte decodes to that byte, and the
        // container's objects are read from what the file holds rather than padded into place.
        let mut dict = Dictionary::new();
        dict.set("Filter", Object::Name(b"RunLengthDecode".to_vec()));
        let stream = Stream::new(dict, vec![5, b'a']);
        assert_eq!(
            decoded(&stream, &BTreeMap::new(), Bounds::stated().decode)
                .expect("the stream decodes"),
            b"a"
        );
    }

    #[test]
    fn a_run_length_stream_without_an_end_marker_decodes_its_runs() {
        // A file whose last run is complete but whose end marker is missing decodes to what it holds:
        // the reference reads its two bytes, ends at the end of the data, and so does this engine.
        let mut dict = Dictionary::new();
        dict.set("Filter", Object::Name(b"RunLengthDecode".to_vec()));
        let stream = Stream::new(dict, vec![2, b'a', b'b', b'c', 254, b'z']);
        assert_eq!(
            decoded(&stream, &BTreeMap::new(), Bounds::stated().decode)
                .expect("the stream decodes"),
            b"abczzz"
        );
    }

    #[test]
    fn a_root_whose_type_is_a_reference_is_not_a_catalog() {
        // `parseDict` builds a catalog only for a dictionary that states `/Type /Catalog` itself, and
        // `maybeRecoverRoot`'s `isValidCatalog` compares what `/Type` looks up to with the name
        // `/Catalog`: a reference naming a dictionary answers the dictionary, so the entry is passed
        // over and the body's own catalog adopted — measured, the reference produces. The walk does
        // the same.
        let file = b"%PDF-1.7\n\
                       1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n\
                       2 0 obj\n<< /Type /Pages /Kids [] /Count 0 >>\nendobj\n\
                       5 0 obj\n<< /Type 1 0 R /Pages 2 0 R >>\nendobj\n\
                       6 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n\
                       trailer\n<< /Root 5 0 R >>\n%%EOF\n";
        let (document, _) = super::document(file, Bounds::stated()).expect("the file reads");
        assert_eq!(
            document
                .trailer
                .get(b"Root")
                .ok()
                .and_then(|root| root.as_reference().ok()),
            Some((6, 0)),
            "the body's own catalog is the one adopted"
        );
    }

    #[test]
    fn an_object_number_past_the_format_is_given_one_of_its_own() {
        // The format puts no ceiling on an object number and the model keys `u32`: the object is kept,
        // under a number of this document's own, and the reference to it follows it there.
        let file = b"%PDF-1.7\n\
                       1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n\
                       2 0 obj\n<< /Type /Pages /Kids [3 0 R] /Count 1 >>\nendobj\n\
                       3 0 obj\n<< /Type /Page /Annots [4294967296 0 R] >>\nendobj\n\
                       4294967296 0 obj\n<< /Type /Annot >>\nendobj\n\
                       trailer\n<< /Root 1 0 R >>\n%%EOF\n";
        let (document, lost) = super::document(file, Bounds::stated()).expect("the file reads");
        assert_eq!(lost.objects, 0);
        let annots = document
            .get_dictionary((3, 0))
            .and_then(|page| page.get(b"Annots"))
            .and_then(Object::as_array)
            .expect("the page states its annotations");
        let Object::Reference(id) = &annots[0] else {
            panic!("the annotation is a reference");
        };
        assert_eq!(
            document
                .get_dictionary(*id)
                .and_then(Dictionary::get_type)
                .ok(),
            Some(b"Annot".as_slice()),
            "the reference follows the object to the number it was given"
        );
    }

    #[test]
    fn an_object_stated_at_a_generation_past_the_format_is_given_a_number_of_its_own() {
        // A generation past 65535 is no generation the model keys, so the object is given a number of
        // this document's own — not the one its header states, which another object may already hold.
        // The reference keys the generation as it stands and renumbers everything when it writes.
        let file = b"%PDF-1.7\n\
                       1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n\
                       2 0 obj\n<< /Type /Pages /Kids [3 65536 R] /Count 1 >>\nendobj\n\
                       3 65536 obj\n<< /Type /Page /MediaBox [0 0 10 10] >>\nendobj\n\
                       trailer\n<< /Root 1 0 R >>\n%%EOF\n";
        let (document, lost) = super::document(file, Bounds::stated()).expect("the file reads");
        assert_eq!(
            lost.objects, 0,
            "nothing is lost: the object is kept under a number of its own"
        );
        let kids = document
            .get_dictionary((2, 0))
            .and_then(|pages| pages.get(b"Kids"))
            .and_then(Object::as_array)
            .expect("the tree states its kids");
        let Object::Reference(id) = &kids[0] else {
            panic!("the kid is a reference");
        };
        assert_eq!(
            document
                .get_dictionary(*id)
                .and_then(Dictionary::get_type)
                .ok(),
            Some(b"Page".as_slice()),
            "the reference follows the object to the number it was given"
        );
    }

    #[test]
    fn two_numbers_longer_than_a_u64_are_reported_as_the_object_they_lose() {
        // The reference reads a run of digits as a double, so these two runs are one number to it and
        // this model cannot key either: one body takes the place of the other, and the answer says the
        // object the first header stated is one the produced file no longer holds.
        let file = b"%PDF-1.7\n\
                       1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n\
                       2 0 obj\n<< /Type /Pages /Kids [] /Count 0 >>\nendobj\n\
                       18446744073709551616 0 obj\nnull\nendobj\n\
                       18446744073709551617 0 obj\nnull\nendobj\n\
                       trailer\n<< /Root 1 0 R >>\n%%EOF\n";
        let (_, lost) = super::document(file, Bounds::stated()).expect("the file reads");
        assert_eq!(lost.objects, 1, "the second object replaced the first");
    }

    #[test]
    fn a_pages_parent_that_is_past_the_model_follows_the_tree() {
        // A page leaf's `/Parent` is not a reference a copy follows, but the rewrite that gives an
        // object past the model its number must rename it all the same: the page states the tree it
        // belongs to, at the number the tree was given.
        let file = b"%PDF-1.7\n\
                       1 0 obj\n<< /Type /Catalog /Pages 5000000000 0 R >>\nendobj\n\
                       5000000000 0 obj\n<< /Type /Pages /Kids [3 0 R] /Count 1 >>\nendobj\n\
                       3 0 obj\n<< /Type /Page /Parent 5000000000 0 R >>\nendobj\n\
                       trailer\n<< /Root 1 0 R >>\n%%EOF\n";
        let (document, lost) = super::document(file, Bounds::stated()).expect("the file reads");
        assert_eq!(lost.objects, 0, "the tree is carried");
        let tree = document
            .get_dictionary((1, 0))
            .and_then(|catalog| catalog.get(b"Pages"))
            .and_then(Object::as_reference)
            .expect("the catalog names its tree");
        assert_eq!(
            document
                .get_dictionary((3, 0))
                .and_then(|page| page.get(b"Parent"))
                .and_then(Object::as_reference)
                .ok(),
            Some(tree),
            "the page states the tree it belongs to"
        );
        assert_eq!(
            document
                .get_dictionary(tree)
                .and_then(Dictionary::get_type)
                .ok(),
            Some(b"Pages".as_slice())
        );
    }

    #[test]
    fn two_numbers_past_a_u64_at_different_generations_are_two_objects() {
        // The two runs are one double to the reference, but they are stated at different generations,
        // and a generation is part of the number: the pairs differ, so the model keeps both — no object
        // is replaced and the answer reports no loss.
        let file = b"%PDF-1.7\n\
                       1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n\
                       2 0 obj\n<< /Type /Pages /Kids [] /Count 0 >>\nendobj\n\
                       18446744073709551616 0 obj\nnull\nendobj\n\
                       18446744073709551617 5 obj\nnull\nendobj\n\
                       trailer\n<< /Root 1 0 R >>\n%%EOF\n";
        let (document, lost) = super::document(file, Bounds::stated()).expect("the file reads");
        assert_eq!(
            lost.objects, 0,
            "the two are two pairs, so nothing is replaced"
        );
        assert_eq!(
            document.inner.objects.len(),
            4,
            "catalog, page tree and both objects the file states past a u64"
        );
    }

    #[test]
    fn two_generations_past_a_u64_that_the_reference_keys_apart_are_two_objects() {
        // The reference reads a generation with `Number` as well, so 2^64 and 2^65 are two generations
        // there and the two bodies are one pair each. Keyed by the run's own double here too, both are
        // kept; a saturated pair would merge them and drop one.
        let file = b"%PDF-1.7\n\
                       1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n\
                       2 0 obj\n<< /Type /Pages /Kids [] /Count 0 >>\nendobj\n\
                       4 18446744073709551616 obj\nnull\nendobj\n\
                       4 36893488147419103232 obj\nnull\nendobj\n\
                       trailer\n<< /Root 1 0 R >>\n%%EOF\n";
        let (_, lost) = super::document(file, Bounds::stated()).expect("the file reads");
        assert_eq!(
            lost.objects, 0,
            "two generations the reference keys apart are two objects"
        );
    }

    #[test]
    fn two_generations_that_round_to_one_double_are_one_object() {
        // 2^64 and 2^64+1 are one double, so the reference keys one pair and the second body replaces
        // the first; the answer reports the object the produced file no longer holds.
        let file = b"%PDF-1.7\n\
                       1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n\
                       2 0 obj\n<< /Type /Pages /Kids [] /Count 0 >>\nendobj\n\
                       4 18446744073709551616 obj\nnull\nendobj\n\
                       4 18446744073709551617 obj\nnull\nendobj\n\
                       trailer\n<< /Root 1 0 R >>\n%%EOF\n";
        let (_, lost) = super::document(file, Bounds::stated()).expect("the file reads");
        assert_eq!(
            lost.objects, 1,
            "the second body replaces the first, and the answer says so"
        );
    }

    #[test]
    fn two_numbers_past_a_u64_that_the_reference_keys_apart_are_two_objects() {
        // A run of digits is a double to the reference, and these two runs are two of them: it keeps
        // both bodies, so this walk must too, each at a number of the document's own. One pair for both
        // would drop an object the file states with nothing said about it.
        let file = b"%PDF-1.7\n\
                       1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n\
                       2 0 obj\n<< /Type /Pages /Kids [3 0 R 4 0 R] /Count 2 >>\nendobj\n\
                       18446744073709551616 0 obj\n<< /Type /Annot /Rect [0 0 1 1] >>\nendobj\n\
                       36893488147419103232 0 obj\n<< /Type /Annot /Rect [1 1 2 2] >>\nendobj\n\
                       trailer\n<< /Root 1 0 R >>\n%%EOF\n";
        let (document, lost) = super::document(file, Bounds::stated()).expect("the file reads");
        assert_eq!(lost.objects, 0, "both objects the file states are kept");
        assert_eq!(
            document
                .objects
                .values()
                .filter(|object| {
                    let kind = object
                        .as_dict()
                        .ok()
                        .and_then(|dictionary| dictionary.get_type().ok());
                    kind == Some(b"Annot".as_slice())
                })
                .count(),
            2,
            "the two are two objects, each placed at a number of the document's own"
        );
        assert_eq!(
            document.inner.objects.len(),
            4,
            "catalog, page tree and the two objects the file states past a u64"
        );
    }

    #[test]
    fn a_number_past_a_u64_stated_twice_with_a_carried_body_loses_the_object_before_it() {
        // The two runs are one double, so this model keys one number; the second body does not parse
        // and is carried as its bytes. The object the first header stated is not in the produced file —
        // the carried bytes are what it holds — and the answer says that rather than letting one body
        // take the place of the other silently.
        let file = b"%PDF-1.7\n\
                       1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n\
                       2 0 obj\n<< /Type /Pages /Kids [] /Count 0 >>\nendobj\n\
                       18446744073709551616 0 obj\n(NEEDLE)\nendobj\n\
                       18446744073709551617 0 obj\n<< broken\nendobj\n\
                       trailer\n<< /Root 1 0 R >>\n%%EOF\n";
        let (document, lost) = super::document(file, Bounds::stated()).expect("the file reads");
        assert_eq!(
            lost.objects, 1,
            "the object the first header stated is one the produced file does not hold"
        );
        let carried: Vec<&Object> = document
            .objects
            .values()
            .filter(|object| graph::carried(object).is_some())
            .collect();
        assert_eq!(carried.len(), 1, "the second body is carried");
        assert_eq!(
            graph::carried(carried[0]),
            Some(&b"<< broken\n"[..]),
            "the carried bytes are the ones the second body stated"
        );
    }

    #[test]
    fn a_containers_entries_are_keyed_by_the_double_their_digits_state() {
        // The reference reads each entry's number as a double, so two runs past a u64 are two objects
        // when they round to two doubles and one when they round to the same: the walk keeps what it
        // keeps, and says which entry it lost.
        for (numbers, joined, lost) in [
            (["18446744073709551616", "36893488147419103232"], 2, 0),
            (["18446744073709551616", "18446744073709551617"], 1, 1),
        ] {
            let file = container_file(numbers);
            let (document, answering) =
                super::document(&file, Bounds::stated()).expect("the file reads");
            assert_eq!(
                answering.objects, lost,
                "{} and {} are {} object(s) to the reference",
                numbers[0], numbers[1], joined
            );
            assert_eq!(
                document.inner.objects.len(),
                2 + joined,
                "the catalog, the page tree and the entries the container's numbers keep"
            );
        }
    }

    /// A file whose one container states two objects at `numbers`, each body an integer.
    fn container_file(numbers: [&str; 2]) -> Vec<u8> {
        use std::fmt::Write as _;
        let mut header = String::new();
        let mut body = String::new();
        for (index, number) in numbers.iter().enumerate() {
            write!(header, "{number} {} ", body.len()).expect("a string writes");
            write!(body, "{}", 11 * (index + 1)).expect("a string writes");
        }
        let first = header.len();
        let data = format!("{header}{body}");
        format!(
            "%PDF-1.7\n\
             1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n\
             2 0 obj\n<< /Type /Pages /Kids [] /Count 0 >>\nendobj\n\
             3 0 obj\n<< /Type /ObjStm /N 2 /First {first} /Length {} >>\n\
             stream\n{data}\nendstream\nendobj\n\
             trailer\n<< /Root 1 0 R >>\n%%EOF\n",
            data.len()
        )
        .into_bytes()
    }

    #[test]
    fn a_remembered_recovery_answers_what_a_fresh_scan_answers() {
        // The record is a reading of the tail, not a guess about it: from every position inside it, a
        // scan holding the record answers what a scan beginning there answers — the same marker, or
        // the same refusal.
        let mut bytes = b"%PDF-1.7\n".to_vec();
        for _ in 0..4 {
            bytes.extend_from_slice(b"<< /Length 3 >>\nstream\nabc\nstream\n");
        }
        // Seven streams opened inside the first stream's bytes and six closed, so a scan from there
        // never balances while one beginning after the first of them does.
        for _ in 0..6 {
            bytes.extend_from_slice(b"endstream\n");
        }
        let from = b"%PDF-1.7\n<< /Length 3 >>\nstream\n".len();
        let mut scan = Scan::new(&bytes, Bounds::stated());
        scan.seek(from);
        assert!(
            scan.end_of_stream().is_err(),
            "no marker in the tail ends the stream"
        );
        let mut record = scan
            .recovery
            .take()
            .expect("the recovery that read to the file's end recorded the tail");
        for at in from..bytes.len() {
            if record.answer(at).is_none() {
                // A token straddles this position, so the record does not speak for it.
                continue;
            }
            let mut fresh = Scan::new(&bytes, Bounds::stated());
            fresh.seek(at);
            let expected = fresh.end_of_stream();
            let mut remembered = Scan::new(&bytes, Bounds::stated());
            remembered.recovery = Some(record);
            remembered.seek(at);
            let answered = remembered.end_of_stream();
            record = remembered
                .recovery
                .take()
                .expect("a scan that begins inside the record keeps it");
            let found = answered.ok();
            let wanted = expected.ok();
            assert_eq!(
                found.is_some(),
                wanted.is_some(),
                "position {at}: the record answers as a fresh scan does"
            );
            assert_eq!(
                found, wanted,
                "position {at}: the record names the same marker"
            );
            if let Some(marker) = found {
                assert_eq!(
                    remembered.at, fresh.at,
                    "position {at}: the scan stands where a fresh one does — past the marker at {marker}"
                );
            }
        }
    }

    #[test]
    fn a_recorded_tail_names_the_token_that_closes_every_level() {
        // The answer is two lookups on the record, not a search of it: every token states the first
        // later token whose nesting is lower — the one that closes the level it left open — or says
        // that no later token does. That is what keeps a file of streams that never end linear.
        let mut bytes = b"%PDF-1.7\n".to_vec();
        for _ in 0..64 {
            bytes.extend_from_slice(b"<< /Length 3 >>\nstream\nabc\nstream\nabc\n");
        }
        for _ in 0..6 {
            bytes.extend_from_slice(b"endstream\n");
        }
        let from = b"%PDF-1.7\n<< /Length 3 >>\nstream\n".len();
        let mut scan = Scan::new(&bytes, Bounds::stated());
        scan.seek(from);
        assert!(scan.end_of_stream().is_err(), "the tail never balances");
        let record = scan.recovery.take().expect("the tail is recorded");
        assert!(record.tokens.len() > 64, "the record holds the whole tail");
        for (index, token) in record.tokens.iter().enumerate() {
            let closer = record.tokens[index + 1..]
                .iter()
                .position(|later| later.nesting < token.nesting)
                .map_or(usize::MAX, |offset| index + 1 + offset);
            assert_eq!(
                token.closer, closer,
                "token {index} names the token that closes it"
            );
        }
    }

    #[test]
    fn a_recorded_tail_answers_every_later_recovery_without_reading() {
        // A file of streams that never end costs one pass over its tail rather than one per stream:
        // after the first failing recovery, every recovery that begins inside the tail is answered
        // from the record and leaves the scan where it was, so no byte of the tail is read again.
        let mut bytes = b"%PDF-1.7\n".to_vec();
        for _ in 0..4000 {
            bytes.extend_from_slice(b"<< /Length 3 >>\nstream\nabc\nstream\n");
        }
        bytes.extend_from_slice(b"x\nendstream\n");
        let from = b"%PDF-1.7\n<< /Length 3 >>\nstream\n".len();
        let mut scan = Scan::new(&bytes, Bounds::stated());
        scan.seek(from);
        assert!(
            scan.end_of_stream().is_err(),
            "the one marker at the end does not end the stream"
        );
        assert_eq!(scan.at, bytes.len(), "the failing recovery read to the end");
        let mut record = scan
            .recovery
            .take()
            .expect("the failing recovery recorded the tail");
        assert_eq!(record.from, from);
        let mut answered = 0;
        for at in (from..bytes.len()).step_by(97) {
            if record.answer(at).is_none() {
                // A token straddles this position, so the record does not speak for it.
                continue;
            }
            let mut known = Scan::new(&bytes, Bounds::stated());
            known.recovery = Some(record);
            known.seek(at);
            let answer = known.end_of_stream();
            record = known.recovery.take().expect("the record is kept");
            if answer.is_err() {
                assert_eq!(
                    known.at, at,
                    "the remembered refusal at {at} read no byte of the tail"
                );
                answered += 1;
            }
        }
        assert!(
            answered > 40,
            "the record answers the file's own positions without reading them"
        );
    }

    #[test]
    fn a_container_that_cannot_be_expanded_is_reported_as_the_loss_it_is() {
        // The container's bytes are carried, so the produced file keeps them unreachable — and the
        // objects it stated it holds are not in the document. Nothing the walk read says how many
        // there were, so the answer repeats the number the file states and attributes it to the file.
        let file = b"%PDF-1.7\n\
                       1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n\
                       2 0 obj\n<< /Type /Pages /Kids [] /Count 0 >>\nendobj\n\
                       3 0 obj\n<< /Type /ObjStm /N 2 /First 8 /Filter /NoSuchDecode /Length 8 >>\n\
                       stream\n4 0 5 3 \nendstream\nendobj\n\
                       trailer\n<< /Root 1 0 R >>\n%%EOF\n";
        let (document, lost) = super::document(file, Bounds::stated()).expect("the file reads");
        assert_eq!(lost.unread, 2, "the container states two objects");
        assert_eq!(
            lost.objects, 0,
            "nothing the walk read was lost: nothing was read"
        );
        assert!(
            document.get_dictionary((3, 0)).is_err(),
            "the container itself is carried as bytes, not expanded"
        );
    }

    #[test]
    fn a_container_count_past_its_own_bytes_is_still_the_files_own_statement() {
        // A damaged container may state any number of objects: the count the answer reports is the one
        // the file states, not a share of its bytes — that number is the file's, and the answer says
        // nothing the file does not. This container decodes; its `/N` names far more entries than the
        // bytes it decodes to could hold, which is where its reading stops.
        let file = b"%PDF-1.7\n\
                       1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n\
                       2 0 obj\n<< /Type /Pages /Kids [] /Count 0 >>\nendobj\n\
                       3 0 obj\n<< /Type /ObjStm /N 1000000 /First 8 /Length 8 >>\n\
                       stream\n4 0 5 3 \nendstream\nendobj\n\
                       trailer\n<< /Root 1 0 R >>\n%%EOF\n";
        let (document, lost) = super::document(file, Bounds::stated()).expect("the file reads");
        assert_eq!(
            lost.unread, 1_000_000,
            "the count is the one the container states"
        );
        assert_eq!(lost.objects, 0, "nothing the walk read was lost");
        assert!(
            document.get_dictionary((3, 0)).is_err(),
            "the container is carried as the bytes it states"
        );
    }

    #[test]
    fn two_container_counts_add_up_to_what_the_two_containers_state() {
        // Both state the largest count a statement holds, and what the answer reports is the sum of
        // what the file states — not a wrapped number and not a failure. Only a sum past what a count
        // can hold saturates, and no file of a size a count can describe reaches that.
        let file = b"%PDF-1.7\n\
                       1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n\
                       2 0 obj\n<< /Type /Pages /Kids [] /Count 0 >>\nendobj\n\
                       3 0 obj\n<< /Type /ObjStm /N 9223372036854775807 /First 0 /Filter /NoSuchDecode /Length 8 >>\n\
                       stream\n4 0 5 3 \nendstream\nendobj\n\
                       4 0 obj\n<< /Type /ObjStm /N 9223372036854775807 /First 0 /Filter /NoSuchDecode /Length 8 >>\n\
                       stream\n4 0 5 3 \nendstream\nendobj\n\
                       trailer\n<< /Root 1 0 R >>\n%%EOF\n";
        let (_, lost) = super::document(file, Bounds::stated()).expect("the file reads");
        assert_eq!(
            lost.unread,
            usize::try_from(u64::MAX - 1).unwrap_or(usize::MAX),
            "the two statements add up to what the file states"
        );
        assert!(!lost.beyond, "two of them fit what a count holds");
    }

    #[test]
    fn a_total_past_what_the_host_holds_is_flagged_rather_than_saturated() {
        // Three containers each claiming the largest count a statement holds: the total is past what a
        // count holds, and the answer says that rather than printing the ceiling as the file's number.
        let mut file = b"%PDF-1.7\n\
                          1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n\
                          2 0 obj\n<< /Type /Pages /Kids [] /Count 0 >>\nendobj\n"
            .to_vec();
        for number in 3..6 {
            let container = format!(
                "{number} 0 obj\n<< /Type /ObjStm /N 9223372036854775807 /First 0 /Filter \
                 /NoSuchDecode /Length 8 >>\nstream\n4 0 5 3 \nendstream\nendobj\n"
            );
            file.extend_from_slice(container.as_bytes());
        }
        file.extend_from_slice(b"trailer\n<< /Root 1 0 R >>\n%%EOF\n");
        let (_, lost) = super::document(&file, Bounds::stated()).expect("the file reads");
        assert!(
            lost.beyond,
            "a total past what a count holds is a fact, not a number to print"
        );
    }

    #[test]
    fn a_count_past_what_the_host_holds_is_flagged_rather_than_printed_as_the_ceiling() {
        // A real `/N` longer than a real can hold reads as infinity, and infinity as a count is the
        // host's ceiling — which is no number the file states. The answer carries the fact instead.
        let file = b"%PDF-1.7\n\
                       1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n\
                       2 0 obj\n<< /Type /Pages /Kids [] /Count 0 >>\nendobj\n\
                       3 0 obj\n<< /Type /ObjStm /N 99999999999999999999999999999999999999999999999.0 \
                       /First 0 /Filter /NoSuchDecode /Length 8 >>\n\
                       stream\n4 0 5 3 \nendstream\nendobj\n\
                       trailer\n<< /Root 1 0 R >>\n%%EOF\n";
        let (_, lost) = super::document(file, Bounds::stated()).expect("the file reads");
        assert!(
            lost.beyond,
            "the count is past what this host holds, and the answer says so rather than printing it"
        );
    }

    #[test]
    fn a_carried_body_at_the_reserved_number_reports_no_broken_reference() {
        // The produced file states no object at number zero, so a reference inside one names nothing
        // whether or not the object it named was placed: counting it would report a loss of a reference
        // the produced file does not state.
        let file = b"%PDF-1.7
                       1 0 obj
<< /Type /Catalog /Pages 2 0 R >>
endobj
                       2 0 obj
<< /Type /Pages /Kids [] /Count 0 >>
endobj
                       0 1 obj
<< /Type /Annot !! broken /Rect 5000000000 0 R >>
endobj
                       5000000000 0 obj
null
endobj
                       trailer
<< /Root 1 0 R >>
%%EOF
";
        let (_, lost) = super::document(file, Bounds::stated()).expect("the file reads");
        assert_eq!(lost.objects, 1, "the object at number zero is the one lost");
        assert_eq!(
            lost.references, 0,
            "the reference is inside a body the produced file does not state"
        );
    }

    #[test]
    fn a_container_whose_first_is_not_a_whole_number_of_bytes_is_carried_and_reported() {
        // `PDFObjectStreamParser` reads `/N` and `/First` with a lookup that requires a number, so a
        // container stating anything else throws and is kept as the bytes it states — with the objects
        // it held gone. No entry was read, so the answer reports the number the file states. A number
        // that is not a whole count of bytes is no position the reference's byte stream can index:
        // measured, a container stating `/First 12.5` stays carried and places nothing, while
        // `/First 12` places both entries.
        for first in [b"/Name".as_slice(), b"12.5"] {
            let file = [
                b"%PDF-1.7\n\
                  1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n\
                  2 0 obj\n<< /Type /Pages /Kids [] /Count 0 >>\nendobj\n\
                  3 0 obj\n<< /Type /ObjStm /N 1 /First "
                    .as_slice(),
                first,
                b" /Length 4 >>\nstream\n4 0 \nendstream\nendobj\n\
                  trailer\n<< /Root 1 0 R >>\n%%EOF\n"
                    .as_slice(),
            ]
            .concat();
            let (document, lost) =
                super::document(&file, Bounds::stated()).expect("the file reads");
            assert_eq!(
                lost.unread,
                1,
                "{:?}: the file states one object in the container",
                String::from_utf8_lossy(first)
            );
            assert_eq!(
                lost.objects, 0,
                "nothing the walk read was lost: nothing was read"
            );
            assert!(
                document.get_dictionary((3, 0)).is_err(),
                "{:?}: the container itself is carried as bytes, not expanded",
                String::from_utf8_lossy(first)
            );
        }
    }

    #[test]
    fn a_negative_first_reads_the_entry_its_offsets_reach() {
        // The reference places an entry at `/First` plus the entry's own offset, so a negative `/First`
        // is no reason to carry the container: the object at that position is the entry. Measured, a
        // container stating `/First -4` whose entry names offset 20 reads the object at byte 16.
        let data = format!("4 20\n{}null", " ".repeat(11));
        let file = [
            b"%PDF-1.7\n\
              1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n\
              2 0 obj\n<< /Type /Pages /Kids [] /Count 0 >>\nendobj\n\
              3 0 obj\n<< /Type /ObjStm /N 1 /First -4 /Length "
                .as_slice(),
            data.len().to_string().as_bytes(),
            b" >>\nstream\n",
            data.as_bytes(),
            b"\nendstream\nendobj\ntrailer\n<< /Root 1 0 R >>\n%%EOF\n",
        ]
        .concat();
        let (document, lost) = super::document(&file, Bounds::stated()).expect("the file reads");
        assert!(
            document.get_object((4, 0)).is_ok(),
            "the entry the offsets reach was placed"
        );
        assert_eq!(lost.objects, 0, "nothing was lost");
        assert_eq!(lost.unread, 0, "the container was read");
    }

    #[test]
    fn a_container_keeps_the_objects_it_read_before_one_that_does_not_parse() {
        // The reference assigns each object to its context as it parses it, so a container whose later
        // object throws keeps the objects read before it — and this walk assigns them the same way.
        let mut dict = Dictionary::new();
        dict.set("Type", Object::Name(b"ObjStm".to_vec()));
        dict.set("N", Object::Integer(2));
        dict.set("First", Object::Integer(8));
        let mut scan = Scan::new(b"", Bounds::stated());
        let mut data = b"4 0 5 3 ".to_vec();
        data.extend_from_slice(b"11 ]");
        let error = scan.object_stream(&Stream::new(dict, data));
        assert!(error.is_err(), "the second object does not parse");
        assert_eq!(scan.objects.get(&(4, 0)), Some(&Object::Integer(11)));
        assert_eq!(
            scan.numbering.lost.objects, 1,
            "one of the two objects it states was kept, so one is lost"
        );
    }

    #[test]
    fn two_objects_the_model_cannot_key_are_given_two_numbers() {
        // Both are referenced before either is stated, so the second must not be given the number the
        // first took: two objects the file states as two stay two.
        let file = b"%PDF-1.7\n\
                       1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n\
                       2 0 obj\n<< /Type /Pages /Kids [3 0 R] /Count 1 >>\nendobj\n\
                       3 0 obj\n<< /Type /Page /Annots [5000000000 0 R 5000000001 0 R] >>\nendobj\n\
                       5000000000 0 obj\n<< /Type /Annot /Rect [0 0 1 1] >>\nendobj\n\
                       5000000001 0 obj\n<< /Type /Annot /Rect [1 1 2 2] >>\nendobj\n\
                       trailer\n<< /Root 1 0 R >>\n%%EOF\n";
        let (document, lost) = super::document(file, Bounds::stated()).expect("the file reads");
        assert_eq!(lost.objects, 0, "both objects are kept");
        let annots = document
            .get_dictionary((3, 0))
            .and_then(|page| page.get(b"Annots"))
            .and_then(Object::as_array)
            .expect("the page states its annotations");
        assert_eq!(annots.len(), 2);
        assert_ne!(annots[0], annots[1], "the two are two objects");
        for annotation in annots {
            let Object::Reference(id) = annotation else {
                panic!("each annotation is a reference");
            };
            assert_eq!(
                document
                    .get_dictionary(*id)
                    .and_then(Dictionary::get_type)
                    .ok(),
                Some(b"Annot".as_slice())
            );
        }
    }

    #[test]
    fn a_reference_read_before_its_object_is_given_a_number_follows_it() {
        // The page names an object the model cannot key before the object itself is stated. The
        // reference is written as the pair's marker until the body has been read, and the number the
        // object is then given is one the file neither states nor names — so the reference follows the
        // object there rather than naming whatever the file put at the number the marker stood at.
        let file = b"%PDF-1.7\n\
                       1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n\
                       2 0 obj\n<< /Type /Pages /Kids [3 0 R] /Count 1 >>\nendobj\n\
                       3 0 obj\n<< /Type /Page /Annots [5000000000 0 R] >>\nendobj\n\
                       5000000000 0 obj\n<< /Type /Annot /Rect [0 0 1 1] >>\nendobj\n\
                       4 0 obj\n<< /Type /Font >>\nendobj\n\
                       trailer\n<< /Root 1 0 R >>\n%%EOF\n";
        let (document, lost) = super::document(file, Bounds::stated()).expect("the file reads");
        assert_eq!(
            lost.objects, 0,
            "the annotation is kept, under another number"
        );
        let annots = document
            .get_dictionary((3, 0))
            .and_then(|page| page.get(b"Annots"))
            .and_then(Object::as_array)
            .expect("the page states its annotations");
        let Object::Reference(id) = &annots[0] else {
            panic!("the annotation is a reference");
        };
        assert_eq!(
            document
                .get_dictionary(*id)
                .and_then(Dictionary::get_type)
                .ok(),
            Some(b"Annot".as_slice()),
            "the reference follows the object to the number it was given"
        );
        assert_eq!(
            document
                .get_dictionary((4, 0))
                .and_then(Dictionary::get_type)
                .ok(),
            Some(b"Font".as_slice()),
            "the object the file stated at 4 is the one at 4"
        );
    }

    #[test]
    fn a_reference_to_a_number_the_file_states_names_the_files_own_object() {
        // The annotation is an object the model cannot key, and the file names its number itself: the
        // number the annotation is given is one past every number the file states or names, so the two
        // never share one — and the page's reference follows the annotation to where it was put.
        let file = b"%PDF-1.7\n\
                       1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n\
                       2 0 obj\n<< /Type /Pages /Kids [3 0 R 5 0 R] /Count 2 >>\nendobj\n\
                       3 0 obj\n<< /Type /Page /Annots [5000000000 0 R] >>\nendobj\n\
                       5 0 obj\n<< /Type /Page /StructParents 4 0 R >>\nendobj\n\
                       5000000000 0 obj\n<< /Type /Annot /Rect [0 0 1 1] >>\nendobj\n\
                       4 0 obj\n<< /Type /Font >>\nendobj\n\
                       trailer\n<< /Root 1 0 R >>\n%%EOF\n";
        let (document, lost) = super::document(file, Bounds::stated()).expect("the file reads");
        assert_eq!(lost.objects, 0, "both objects are kept");
        let kind = |id: ObjectId| {
            document
                .get_dictionary(id)
                .and_then(Dictionary::get_type)
                .ok()
                .map(<[u8]>::to_vec)
        };
        let Object::Reference(annot) = &document
            .get_dictionary((3, 0))
            .and_then(|page| page.get(b"Annots"))
            .and_then(Object::as_array)
            .unwrap()[0]
        else {
            panic!("the annotation is a reference");
        };
        assert_eq!(
            kind(*annot),
            Some(b"Annot".to_vec()),
            "the page's reference follows the annotation to the number it was given"
        );
        let Object::Reference(named) = document
            .get_dictionary((5, 0))
            .and_then(|page| page.get(b"StructParents"))
            .expect("the entry reads")
        else {
            panic!("the entry is a reference");
        };
        assert_eq!(
            kind(*named),
            Some(b"Font".to_vec()),
            "a reference to the number the file states names the file's own object"
        );
    }

    #[test]
    fn a_dangling_reference_past_the_model_is_written_at_a_marker() {
        // A reference to a number the model cannot key has no id to be written back at, so it is
        // written at the marker its pair is keyed by — a generation no object is ever placed at, so it
        // names nothing, as the file left it, whichever number the marker stands at.
        let file = b"%PDF-1.7\n\
                       1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n\
                       2 0 obj\n<< /Type /Pages /Kids [3 0 R] /Count 1 >>\nendobj\n\
                       3 0 obj\n<< /Type /Page /Annots [5000000001 0 R] >>\nendobj\n\
                       5000000000 0 obj\n<< /Type /Annot /Rect [0 0 1 1] >>\nendobj\n\
                       trailer\n<< /Root 1 0 R >>\n%%EOF\n";
        let (document, _) = super::document(file, Bounds::stated()).expect("the file reads");
        let annots = document
            .get_dictionary((3, 0))
            .and_then(|page| page.get(b"Annots"))
            .and_then(Object::as_array)
            .expect("the page states its annotations");
        let Object::Reference(named) = annots[0] else {
            panic!("the annotation is a reference");
        };
        assert_eq!(
            named.1,
            u16::MAX,
            "a marker states the format's highest generation"
        );
        assert!(
            document.get_dictionary(named).is_err(),
            "nothing was put at the marker's number"
        );
    }

    #[test]
    fn a_reference_the_file_does_not_hold_stays_dangling() {
        // The page names an object the file never states. A number given to an object the model cannot
        // key is one past every number the file states or references, so the dangling reference is not
        // resolved to it.
        let file = b"%PDF-1.7\n\
                       1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n\
                       2 0 obj\n<< /Type /Pages /Kids [3 0 R] /Count 1 >>\nendobj\n\
                       3 0 obj\n<< /Type /Page /Annots [4 0 R] >>\nendobj\n\
                       5000000000 0 obj\n<< /Type /Annot /Rect [0 0 1 1] >>\nendobj\n\
                       trailer\n<< /Root 1 0 R >>\n%%EOF\n";
        let (document, _) = super::document(file, Bounds::stated()).expect("the file reads");
        let annots = document
            .get_dictionary((3, 0))
            .and_then(|page| page.get(b"Annots"))
            .and_then(Object::as_array)
            .expect("the page states its annotations");
        assert_eq!(
            annots[0],
            Object::Reference((4, 0)),
            "the reference names what the file named"
        );
        assert!(
            document.get_dictionary((4, 0)).is_err(),
            "nothing was put at the number the file left dangling"
        );
    }

    #[test]
    fn a_number_inside_a_carried_bodys_string_or_comment_is_not_a_reference() {
        // A carried body is read by a keyword scan rather than a parser, so what it steps over is the
        // difference between a reference and three numbers that lie together: a literal string, a
        // hexadecimal string, a comment and a name state no reference, and a number one of them holds
        // costs the answer nothing.
        let file = b"%PDF-1.7
\
                       1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n\
                       2 0 obj\n<< /Type /Pages /Kids [] /Count 0 >>\nendobj\n\
                       5000000000 0 obj\n<< /Type /Annot /Rect [0 0 1 1] >>\nendobj\n\
                       200 0 obj\n<< /Type /Annot /Note (5000000000 0 R) /Hex <5000000000> \n\
                       % 5000000000 0 R\n /5000000000 0 R !! broken\nendobj\n\
                       trailer\n<< /Root 1 0 R >>\n%%EOF\n";
        let (document, lost) = super::document(file, Bounds::stated()).expect("the file reads");
        assert_eq!(
            lost.references, 0,
            "the carried body states no reference to the placed object"
        );
        assert!(
            graph::carried(document.objects.get(&(200, 0)).expect("the carried body")).is_some(),
            "the body is carried as the bytes it states"
        );
    }

    #[test]
    fn the_same_number_longer_than_a_u64_stated_twice_loses_nothing() {
        // An incremental save states the same object twice, which is a redefinition and not a loss.
        let file = b"%PDF-1.7\n\
                       1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n\
                       2 0 obj\n<< /Type /Pages /Kids [] /Count 0 >>\nendobj\n\
                       18446744073709551616 0 obj\nnull\nendobj\n\
                       18446744073709551616 0 obj\nnull\nendobj\n\
                       trailer\n<< /Root 1 0 R >>\n%%EOF\n";
        let (_, lost) = super::document(file, Bounds::stated()).expect("the file reads");
        assert_eq!(
            lost.objects, 0,
            "the same object stated twice loses nothing"
        );
    }

    #[test]
    fn the_same_number_spelled_with_a_leading_zero_is_a_redefinition() {
        // `05000000000` is the number `5000000000`, and the reference keys both as one object: the
        // second body replaces the first, so nothing the input states is missing from the produced
        // file that the reference would hold either.
        let file = b"%PDF-1.7\n\
                       1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n\
                       2 0 obj\n<< /Type /Pages /Kids [] /Count 0 >>\nendobj\n\
                       5000000000 0 obj\nnull\nendobj\n\
                       05000000000 0 obj\nnull\nendobj\n\
                       trailer\n<< /Root 1 0 R >>\n%%EOF\n";
        let (_, lost) = super::document(file, Bounds::stated()).expect("the file reads");
        assert_eq!(lost.objects, 0, "one number stated twice loses nothing");
    }

    #[test]
    fn a_containers_entry_and_a_header_stating_one_pair_lose_nothing() {
        // The entry's digits and the header's state the same number and generation — the entry states
        // no generation, and the header states the zero every entry implies — so the header finds the
        // body the container placed and replaces it: one value, one object, nothing lost.
        let file = b"%PDF-1.7\n\
                       1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n\
                       2 0 obj\n<< /Type /Pages /Kids [] /Count 0 >>\nendobj\n\
                       3 0 obj\n<< /Type /ObjStm /N 1 /First 3 /Length 16 >>\n\
                       stream\n5000000000 0 null\nendstream\nendobj\n\
                       5000000000 0 obj\nnull\nendobj\n\
                       trailer\n<< /Root 1 0 R >>\n%%EOF\n";
        let (_, lost) = super::document(file, Bounds::stated()).expect("the file reads");
        assert_eq!(lost.objects, 0, "one pair stated twice loses nothing");
    }

    #[test]
    fn a_reference_the_file_states_at_a_markers_number_stays_dangling() {
        // A marker states the format's highest generation, and this file names that very number
        // without stating an object there. The marker is a pair the map holds, so the file's own
        // meaning is the one it answers: the reference names nothing, as the file left it, and the
        // annotation it also names is carried at a number of the document's own.
        let file = b"%PDF-1.7\n\
                       1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n\
                       2 0 obj\n<< /Type /Pages /Kids [3 0 R] /Count 1 >>\nendobj\n\
                       3 0 obj\n<< /Type /Page /Annots [4294967295 65535 R 5000000000 0 R] >>\nendobj\n\
                       5000000000 0 obj\n<< /Type /Annot /Rect [0 0 1 1] >>\nendobj\n\
                       trailer\n<< /Root 1 0 R >>\n%%EOF\n";
        let (document, lost) = super::document(file, Bounds::stated()).expect("the file reads");
        assert_eq!(
            lost.objects, 0,
            "the annotation is carried under another number"
        );
        let annots = document
            .get_dictionary((3, 0))
            .and_then(|page| page.get(b"Annots"))
            .and_then(Object::as_array)
            .expect("the page states its annotations");
        assert_eq!(
            annots[0],
            Object::Reference((u32::MAX, u16::MAX)),
            "the reference names what the file named"
        );
        assert!(
            document.get_dictionary((u32::MAX, u16::MAX)).is_err(),
            "nothing was put at the number the file left dangling"
        );
        let Object::Reference(annot) = annots[1] else {
            panic!("the annotation is a reference");
        };
        assert_eq!(
            document
                .get_dictionary(annot)
                .and_then(Dictionary::get_type)
                .ok(),
            Some(b"Annot".as_slice()),
            "the annotation follows the number it was given"
        );
    }

    #[test]
    fn an_object_the_file_states_at_a_markers_number_is_carried() {
        // Markers state the format's highest generation, so an object the file states there is held
        // by its pair like one past the model rather than keyed by an id a marker already stands at:
        // both objects are carried, and neither replaces the other.
        let file = b"%PDF-1.7\n\
                       1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n\
                       2 0 obj\n<< /Type /Pages /Kids [3 0 R] /Count 1 >>\nendobj\n\
                       3 0 obj\n<< /Type /Page /Annots [5000000000 0 R] /StructParents 4294967295 65535 R >>\nendobj\n\
                       4294967295 65535 obj\n<< /Type /Font >>\nendobj\n\
                       5000000000 0 obj\n<< /Type /Annot /Rect [0 0 1 1] >>\nendobj\n\
                       trailer\n<< /Root 1 0 R >>\n%%EOF\n";
        let (document, lost) = super::document(file, Bounds::stated()).expect("the file reads");
        assert_eq!(lost.objects, 0, "both objects are kept");
        let kind = |id: ObjectId| {
            document
                .get_dictionary(id)
                .and_then(Dictionary::get_type)
                .ok()
                .map(<[u8]>::to_vec)
        };
        let page = document.get_dictionary((3, 0)).expect("the page");
        let Object::Reference(font) = page.get(b"StructParents").expect("the entry reads") else {
            panic!("the entry is a reference");
        };
        assert_eq!(
            kind(*font),
            Some(b"Font".to_vec()),
            "the reference follows the object the file stated at that number"
        );
        let Object::Reference(annot) = page
            .get(b"Annots")
            .ok()
            .and_then(|annots| annots.as_array().ok())
            .expect("the page states its annotations")[0]
        else {
            panic!("the annotation is a reference");
        };
        assert_eq!(
            kind(annot),
            Some(b"Annot".to_vec()),
            "the object past the model is carried too"
        );
        assert_ne!(*font, annot, "the two are two objects");
    }

    #[test]
    fn a_dangling_reference_keeps_the_number_the_file_stated() {
        // The object past the model is read first, so the walk's first marker stands at the number the
        // page names without stating an object there. That reference is written back at the id the file
        // used — the number it named at the generation it stated — rather than at the marker's number,
        // which is one the file never states and the produced file must not invent.
        let file = b"%PDF-1.7\n\
                       1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n\
                       2 0 obj\n<< /Type /Pages /Kids [3 0 R] /Count 1 >>\nendobj\n\
                       5000000000 0 obj\n<< /Type /Annot /Rect [0 0 1 1] >>\nendobj\n\
                       3 0 obj\n<< /Type /Page /Annots [4294967295 65535 R 5000000000 0 R] >>\nendobj\n\
                       trailer\n<< /Root 1 0 R >>\n%%EOF\n";
        let (document, _) = super::document(file, Bounds::stated()).expect("the file reads");
        let annots = document
            .get_dictionary((3, 0))
            .and_then(|page| page.get(b"Annots"))
            .and_then(Object::as_array)
            .expect("the page states its annotations");
        assert_eq!(
            annots[0],
            Object::Reference((u32::MAX, u16::MAX)),
            "the dangling reference names the number the file named"
        );
        assert!(
            document.get_dictionary((u32::MAX, u16::MAX)).is_err(),
            "nothing was put at the number the file left dangling"
        );
        let Object::Reference(annot) = annots[1] else {
            panic!("the annotation is a reference");
        };
        assert_ne!(
            annot,
            (u32::MAX, u16::MAX),
            "the placed object did not take the marker's number"
        );
        assert_eq!(
            document
                .get_dictionary(annot)
                .and_then(Dictionary::get_type)
                .ok(),
            Some(b"Annot".as_slice()),
            "the annotation follows the number it was given"
        );
    }

    #[test]
    fn a_container_numbers_its_objects_with_the_document_they_join() {
        // The document states an object the model cannot key, and an object inside a container names
        // it. The container's walk shares the document's numbering, so the reference it states follows
        // the object the document put there instead of being given a second number of its own.
        let held = b"5 0 << /Type /Font /BaseFont 5000000000 0 R >>";
        let mut file = b"%PDF-1.7\n\
                       1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n\
                       2 0 obj\n<< /Type /Pages /Kids [] /Count 0 >>\nendobj\n\
                       5000000000 0 obj\n<< /Type /Annot /Rect [0 0 1 1] >>\nendobj\n"
            .to_vec();
        file.extend_from_slice(
            format!(
                "4 0 obj\n<< /Type /ObjStm /N 1 /First 4 /Length {} >>\nstream\n",
                held.len()
            )
            .as_bytes(),
        );
        file.extend_from_slice(held);
        file.extend_from_slice(b"\nendstream\nendobj\ntrailer\n<< /Root 1 0 R >>\n%%EOF\n");
        let (document, lost) = super::document(&file, Bounds::stated()).expect("the file reads");
        assert_eq!(lost.objects, 0, "both objects are kept");
        let Object::Reference(font) = document
            .get_dictionary((5, 0))
            .and_then(|held| held.get(b"BaseFont"))
            .expect("the container's object states its font")
        else {
            panic!("the entry is a reference");
        };
        assert_ne!(
            *font,
            (4, 0),
            "the reference does not point at its own container"
        );
        assert_eq!(
            document
                .get_dictionary(*font)
                .and_then(Dictionary::get_type)
                .ok(),
            Some(b"Annot".as_slice()),
            "the reference names the object the file states"
        );
    }

    #[test]
    fn a_number_stated_again_after_another_loses_the_object_between_them() {
        // A, B, A: the three runs round to one double, so they are one number to the reference and this
        // model cannot key any of them. Each header that states a different run takes the place of the
        // body before it — two objects the produced file does not hold — while the third header states
        // the first run again, which is a redefinition rather than a replacement.
        let file = b"%PDF-1.7\n\
                       1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n\
                       2 0 obj\n<< /Type /Pages /Kids [] /Count 0 >>\nendobj\n\
                       18446744073709551616 0 obj\nnull\nendobj\n\
                       18446744073709551617 0 obj\nnull\nendobj\n\
                       18446744073709551616 0 obj\nnull\nendobj\n\
                       trailer\n<< /Root 1 0 R >>\n%%EOF\n";
        let (_, lost) = super::document(file, Bounds::stated()).expect("the file reads");
        assert_eq!(
            lost.objects, 2,
            "each header that states a different run replaces one"
        );
    }

    #[test]
    fn a_run_length_stream_decodes_to_as_many_bytes_as_it_states() {
        // Nine runs of 128 bytes, stated by eighteen input bytes, are all decoded: the ceiling is the
        // typed array's own, far past any chain a file states.
        let mut dict = Dictionary::new();
        dict.set("Filter", Object::Name(b"RunLengthDecode".to_vec()));
        let mut data = Vec::new();
        for _ in 0..=8 {
            data.extend_from_slice(&[0x81, b'z']);
        }
        data.push(0x80);
        let stream = Stream::new(dict, data);
        assert_eq!(
            decoded(&stream, &BTreeMap::new(), Bounds::stated().decode)
                .expect("the stream decodes")
                .len(),
            9 * 128
        );
    }

    #[test]
    fn a_flate_stage_past_the_ceiling_is_not_decoded() {
        // Flate cannot count its output before it builds it, so the ceiling is measured as the stage
        // produces: past it the stage is abandoned and the container carried, and under it the same
        // bytes decode.
        use std::io::Write as _;
        let payload = vec![b'z'; 4096];
        let mut encoder =
            flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
        encoder.write_all(&payload).expect("a writer");
        let compressed = encoder.finish().expect("a stream");
        let mut dict = Dictionary::new();
        dict.set("Filter", Object::Name(b"FlateDecode".to_vec()));
        let stream = Stream::new(dict, compressed);
        assert!(decoded(&stream, &BTreeMap::new(), 1024).is_none());
        assert_eq!(
            decoded(&stream, &BTreeMap::new(), 8192).map(|bytes| bytes.len()),
            Some(4096)
        );
    }

    #[test]
    fn an_early_change_entry_is_read_through_the_object_map() {
        // The entry the reference's decoder does read, resolved through the object map as `lookup`
        // resolves it: a stream encoded with the early change and read with the standard switch is not
        // the bytes it states.
        let payload: Vec<u8> = (0..=255_u8).chain(0..=255).collect();
        let data = crate::test_support::build::lzw(&payload);
        let stream = |parms: Option<Dictionary>| {
            let mut dict = Dictionary::new();
            dict.set("Filter", Object::Name(b"LZWDecode".to_vec()));
            if let Some(parms) = parms {
                dict.set("DecodeParms", Object::Dictionary(parms));
            }
            Stream::new(dict, data.clone())
        };
        let mut objects = BTreeMap::new();
        objects.insert((9, 0), Object::Integer(1));
        let mut parms = Dictionary::new();
        parms.set("EarlyChange", Object::Reference((9, 0)));
        let read = decoded(&stream(Some(parms)), &objects, Bounds::stated().decode);
        assert_eq!(read.as_deref(), Some(payload.as_slice()));
        let mut parms = Dictionary::new();
        parms.set("EarlyChange", Object::Integer(0));
        assert_ne!(
            decoded(
                &stream(Some(parms)),
                &BTreeMap::new(),
                Bounds::stated().decode
            )
            .as_deref(),
            Some(payload.as_slice())
        );
    }

    #[test]
    fn a_container_past_the_ceiling_is_carried_rather_than_refused() {
        // The file is read and its container kept as the bytes it states, with the objects it claims
        // reported as the loss they are: a bound on what a container may cost is not a refusal of the
        // file that holds it.
        let (document, lost) = super::document(
            &crate::test_support::build::pdf_object_stream_lzw(),
            Bounds {
                decode: 8,
                ..Bounds::stated()
            },
        )
        .expect("the file is read");
        assert!(lost.unread > 0, "the container's claim is reported");
        assert!(
            !document.objects.contains_key(&(8, 0)),
            "the objects inside the container are not placed"
        );
        let (document, lost) = super::document(
            &crate::test_support::build::pdf_object_stream_lzw(),
            Bounds::stated(),
        )
        .expect("the file is read");
        assert_eq!(lost.unread, 0);
        assert!(
            document.objects.contains_key(&(8, 0)),
            "under the ceiling the container is expanded"
        );
    }

    #[test]
    fn a_container_whose_body_decodes_to_a_zero_block_is_expanded_rather_than_carried() {
        // An empty `LZWDecode` body decodes to the block of zero codes the reference pads a stream
        // with, so the container is expanded and its entries are read: none parses, nothing is placed,
        // and the objects it claims are the walk's own count rather than the file's unread claim.
        let (document, lost) = super::document(
            &crate::test_support::build::pdf_object_stream_empty_lzw(),
            Bounds::stated(),
        )
        .expect("the file is read");
        assert_eq!(lost.unread, 0, "the container is not carried unread");
        assert_eq!(lost.objects, 5, "the objects it states are counted as lost");
        assert!(
            !document.objects.contains_key(&(8, 0)),
            "nothing inside a zero block is placed"
        );
    }

    #[test]
    fn a_flate_stream_is_read_up_to_its_last_block() {
        // The reference's decoder reads deflate blocks and stops at the last one; it never reads the
        // checksum after them. So a stream missing only its checksum decodes whole, and one whose last
        // block is cut short is not decoded at all — the container is kept as the bytes it states.
        use std::io::Write as _;
        let payload = b"a container's bytes";
        let mut encoder =
            flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
        encoder.write_all(payload).expect("a writer");
        let compressed = encoder.finish().expect("a stream");
        let mut dict = Dictionary::new();
        dict.set("Filter", Object::Name(b"FlateDecode".to_vec()));
        let whole = |bytes: &[u8]| {
            decoded(
                &Stream::new(dict.clone(), bytes.to_vec()),
                &BTreeMap::new(),
                Bounds::stated().decode,
            )
        };
        assert_eq!(whole(&compressed).as_deref(), Some(payload.as_slice()));
        assert_eq!(
            whole(&compressed[..compressed.len() - 4]).as_deref(),
            Some(payload.as_slice()),
            "the checksum is not read"
        );
        assert!(
            whole(&compressed[..2]).is_none(),
            "a stream cut before its last block is not decoded"
        );
        assert!(
            whole(b"not a zlib stream").is_none(),
            "a stream without a zlib header is not decoded"
        );
    }

    #[test]
    fn a_filter_chain_past_the_ceiling_is_not_decoded() {
        // Two RunLength stages: the first decodes to a 33-byte RunLength stream, the second to its
        // 2 048 bytes. Under a one-kibibyte ceiling the second stage is never built, so the container
        // is carried rather than the file refused; under four kibibytes the same chain decodes, which
        // is what the engine ships, the typed array's own ceiling being far past it.
        let mut inner = Vec::new();
        for _ in 0..16 {
            inner.extend_from_slice(&[0x81, b'z']);
        }
        inner.push(0x80);
        let mut data = vec![u8::try_from(inner.len() - 1).expect("a literal run")];
        data.extend_from_slice(&inner);
        data.push(0x80);
        let mut dict = Dictionary::new();
        dict.set(
            "Filter",
            Object::Array(vec![
                Object::Name(b"RunLengthDecode".to_vec()),
                Object::Name(b"RunLengthDecode".to_vec()),
            ]),
        );
        let stream = Stream::new(dict, data);
        assert!(decoded(&stream, &BTreeMap::new(), 1024).is_none());
        assert_eq!(
            decoded(&stream, &BTreeMap::new(), 4096).map(|bytes| bytes.len()),
            Some(16 * 128)
        );
    }

    #[test]
    fn an_index_stream_whose_table_is_truncated_still_states_its_entries() {
        // The reference states the trailer entries before it reads the table, and past the end of the
        // bytes it reads zeros rather than refusing, so a truncated table loses nothing it stated.
        let mut dict = Dictionary::new();
        dict.set("Type", Object::Name(b"XRef".to_vec()));
        dict.set("Size", Object::Integer(4));
        dict.set("W", Object::Array(vec![Object::Integer(1)]));
        dict.set("Root", Object::Reference((1, 0)));
        let mut scan = Scan::new(b"", Bounds::stated());
        scan.index_stream(&Stream::new(dict, vec![1, 2]));
        assert_eq!(
            scan.trailer.get(b"Root").ok(),
            Some(&Object::Reference((1, 0)))
        );
    }

    #[test]
    fn a_filter_stated_indirectly_is_the_filter_it_names() {
        // The reference reads `/Filter` with `lookup`, so a filter chain the dictionary states by
        // reference is the chain it names — not a stream with no filter at all.
        let mut objects = BTreeMap::new();
        objects.insert((1, 0), Object::Name(b"ASCIIHexDecode".to_vec()));
        objects.insert(
            (2, 0),
            Object::Array(vec![Object::Name(b"ASCIIHexDecode".to_vec())]),
        );
        for filter in [Object::Reference((1, 0)), Object::Reference((2, 0))] {
            let mut dict = Dictionary::new();
            dict.set("Filter", filter);
            let stream = Stream::new(dict, b"616263".to_vec());
            assert_eq!(
                decoded(&stream, &objects, Bounds::stated().decode).expect("the stream decodes"),
                b"abc"
            );
        }
    }

    #[test]
    fn an_index_stream_whose_index_is_not_an_array_states_its_entries_anyway() {
        // The reference falls back to `[0 Size]` when `/Index` is not an array, so such a container
        // still states its trailer entries — including the `/Encrypt` a damaged file must not lose.
        let mut dict = Dictionary::new();
        dict.set("Type", Object::Name(b"XRef".to_vec()));
        dict.set("Size", Object::Integer(1));
        dict.set("W", Object::Array(vec![Object::Integer(1)]));
        dict.set("Index", Object::Name(b"Index".to_vec()));
        dict.set("Encrypt", Object::Reference((2, 0)));
        let mut scan = Scan::new(b"", Bounds::stated());
        scan.index_stream(&Stream::new(dict, Vec::new()));
        assert_eq!(
            scan.trailer.get(b"Encrypt").ok(),
            Some(&Object::Reference((2, 0)))
        );
    }

    #[test]
    fn an_object_deeper_than_the_scan_builds_is_carried_as_its_own_bytes() {
        // The reference's parser keeps an object as its bytes once its own stack runs out, and so does
        // this walk: the file is not refused, and the bytes are the ones the object stated.
        let mut scan = Scan::new(
            b"1 0 obj\n[[[[[1]]]]]\nendobj\n",
            Bounds {
                depth: 4,
                ..Bounds::stated()
            },
        );
        scan.indirect_objects().expect("the object is carried");
        let object = scan.objects.get(&(1, 0)).expect("the object is kept");
        assert_eq!(
            object.as_stream().expect("carried as a stream").content,
            b"[[[[[1]]]]]\n"
        );
    }

    #[test]
    fn a_body_read_later_is_the_body_the_file_states_last() {
        // An object first defined too deep and then redefined with a body that parses is the body the
        // file states last, as it is for the reference: the walk takes the last definition it meets.
        let bytes = b"1 0 obj\n[[[[[1]]]]]\nendobj\n1 0 obj\n7\nendobj\n";
        let mut scan = Scan::new(
            bytes,
            Bounds {
                depth: 4,
                ..Bounds::stated()
            },
        );
        scan.indirect_objects().expect("the objects are read");
        assert_eq!(scan.objects.get(&(1, 0)), Some(&Object::Integer(7)));
    }

    #[test]
    fn a_look_gives_up_after_its_own_allowance() {
        // A look inspects at most `scan_look_bytes_max` bytes: a header whose comment is longer than
        // that is not found, and the scan is left where it began.
        let max = vocabulary().engine.scan_look_bytes_max;
        let mut header = b"1 0 %".to_vec();
        header.extend(std::iter::repeat_n(b'x', max + 1));
        header.extend_from_slice(b"\nobj");
        let mut scan = Scan::new(&header, Bounds::stated());
        assert!(!scan.look(|scan| scan.indirect_object_header().map(|_| true)));
        assert_eq!(scan.at, 0);
    }

    #[test]
    fn an_object_stream_whose_count_and_first_are_indirect_is_expanded() {
        // The reference resolves `/N` and `/First` through the object it names, so a container that
        // states them indirectly is still a container.
        let mut scan = Scan::new(b"", Bounds::stated());
        let (bytes, runs) = held("1");
        scan.assign(Key::Held((1, 0)), bytes, runs, Object::Integer(2));
        let (bytes, runs) = held("2");
        scan.assign(Key::Held((2, 0)), bytes, runs, Object::Integer(8));
        let mut data = b"4 0 5 3 ".to_vec();
        data.extend_from_slice(b"11 22");
        let mut dict = Dictionary::new();
        dict.set("Type", Object::Name(b"ObjStm".to_vec()));
        dict.set("N", Object::Reference((1, 0)));
        dict.set("First", Object::Reference((2, 0)));
        scan.object_stream(&Stream::new(dict, data))
            .expect("the container is read");
        assert_eq!(scan.objects.get(&(4, 0)), Some(&Object::Integer(11)));
        assert_eq!(scan.objects.get(&(5, 0)), Some(&Object::Integer(22)));
    }

    #[test]
    fn a_run_of_digits_longer_than_a_u64_saturates_rather_than_failing() {
        // The reference reads such a run as a number, so it is one here too: an object numbered past
        // what the model keys is given a number of its own rather than refused. The run is read whole
        // and saturates at the ceiling; what keeps the work bounded is that a look at what comes next
        // reads at most its own allowance, so the run is not re-read from every byte it passes.
        let mut scan = Scan::new(b"99999999999999999999999", Bounds::stated());
        assert_eq!(scan.raw_int().ok(), Some(u64::MAX));
        assert_eq!(scan.at, 23, "the whole run was read");
    }

    #[test]
    fn a_name_is_decoded_the_way_the_reference_decodes_one() {
        // The reference decodes `#` followed by two upper-case hexadecimal digits, and only that.
        assert_eq!(decode_name(b"Ty#70e".to_vec()), b"Type");
        assert_eq!(decode_name(b"Ty#7ae".to_vec()), b"Ty#7ae");
        assert_eq!(decode_name(b"Ty#7".to_vec()), b"Ty#7");
        assert_eq!(decode_name(b"A#20B".to_vec()), b"A B");
    }

    #[test]
    fn an_indirect_type_names_a_container() {
        // The reference resolves `/Type` through the object it names, so a container whose type is
        // stated indirectly is still a container.
        let mut scan = Scan::new(b"", Bounds::stated());
        let (bytes, runs) = held("1");
        scan.assign(
            Key::Held((1, 0)),
            bytes,
            runs,
            Object::Name(b"ObjStm".to_vec()),
        );
        let mut dict = Dictionary::new();
        dict.set("Type", Object::Reference((1, 0)));
        assert_eq!(resolved_type(&dict, &scan.objects), Some(&b"ObjStm"[..]));
    }

    #[test]
    fn the_decode_parameters_follow_the_shape_of_the_chain() {
        // The reference reads a chain of one filter's parameters as they stand — an array states none —
        // and a chain of several at each filter's own place, so a single dictionary states none for
        // either filter. Only LZW reads them, through its `EarlyChange`.
        let early = Dictionary::from_iter([("EarlyChange", Object::Integer(0))]);
        let single = Dictionary::from_iter([
            ("Filter", Object::Name(b"LZWDecode".to_vec())),
            (
                "DecodeParms",
                Object::Array(vec![Object::Dictionary(early.clone())]),
            ),
        ]);
        let chain = Dictionary::from_iter([
            (
                "Filter",
                Object::Array(vec![
                    Object::Name(b"LZWDecode".to_vec()),
                    Object::Name(b"FlateDecode".to_vec()),
                ]),
            ),
            ("DecodeParms", Object::Dictionary(early.clone())),
        ]);
        let objects = BTreeMap::new();
        assert_eq!(
            filters(&Stream::new(single, Vec::new()), &objects),
            Some(vec![(b"LZWDecode".to_vec(), None)])
        );
        assert_eq!(
            filters(&Stream::new(chain, Vec::new()), &objects),
            Some(vec![
                (b"LZWDecode".to_vec(), None),
                (b"FlateDecode".to_vec(), None),
            ])
        );
    }

    #[test]
    fn a_stream_keyword_may_end_its_line_either_way() {
        for (bytes, content) in [
            (&b"<< /Length 3 >>\nstream\nabc\nendstream"[..], &b"abc"[..]),
            (
                &b"<< /Length 3 >>\r\nstream\r\nabc\r\nendstream"[..],
                &b"abc"[..],
            ),
            (&b"<< /Length 3 >>\rstream\rabc\rendstream"[..], &b"abc"[..]),
            // The bare keyword takes no end of line with it, so the length counts from the byte
            // after the keyword itself.
            (&b"<< /Length 4 >> stream abc endstream"[..], &b" abc"[..]),
        ] {
            let mut scan = Scan::new(bytes, Bounds::stated());
            let Object::Stream(stream) = scan.object(scan.bounds.depth).expect("a stream") else {
                panic!("a stream was not read as one");
            };
            assert_eq!(
                stream.content,
                content,
                "{:?}",
                String::from_utf8_lossy(bytes)
            );
        }
    }

    #[test]
    fn a_hexadecimal_string_with_an_odd_digit_count_is_padded() {
        let mut scan = Scan::new(b"<414>", Bounds::stated());
        let Object::String(bytes, format) = scan.object(scan.bounds.depth).expect("a string")
        else {
            panic!("a string was not read as one");
        };
        assert_eq!(bytes, vec![0x41, 0x40]);
        assert_eq!(format, StringFormat::Hexadecimal);
    }
}

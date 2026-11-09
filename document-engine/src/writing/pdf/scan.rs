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
//! An object stream the walk cannot expand is kept unread, and the writer refuses the file rather than
//! write one missing what the container held; an index stream it cannot decode is dropped, because the
//! only thing such a stream holds is an index the writer rebuilds.
//!
//! The walk refuses what the reference's parser refuses, and that is the point of reproducing it
//! rather than repairing an index: a header, an index section, a trailer or an object header whose
//! syntax does not parse — a cross-reference entry that starts with a digit and then states something
//! else, an unterminated trailer dictionary — refuses it, even though the body is otherwise intact.
//! A missing `endobj` is not one of those: the reference matches that keyword and carries on when it
//! is absent, and so does the walk. An object whose *body* does not parse is another matter: the
//! walk keeps it as an unread stream, and the writer refuses the file rather than write one that
//! lost those bytes.
//!
//! The walk is bounded: it may read at most
//! [`pdf_scan_passes_max`](crate::vocabulary::EngineBounds::pdf_scan_passes_max) times the file's own
//! length, so a file that makes the recovery read it over and over is refused instead of scanned
//! indefinitely. A look at what comes next is not a read of it: [`Scan::look`] inspects at most
//! [`scan_look_bytes_max`](crate::vocabulary::EngineBounds::scan_look_bytes_max) bytes of its own, so a
//! body built of long comment or digit runs costs at most that many reads per byte of its own — linear,
//! but that many times the reads the pass budget charges. An object stream's own bytes are bounded the
//! same way, against their own length once decoded, because a container holds far more than the bytes
//! that state it. What a container may decode to is bounded by
//! [`container_decode_max_bytes`](crate::vocabulary::EngineBounds::container_decode_max_bytes), and
//! [`decoded`] states how the ceiling meets each decoder. A container past the ceiling is refused with
//! the engine's own sentence; the reference has no such bound.
//!
//! The walk recurses no deeper than
//! [`object_depth_max`](crate::vocabulary::EngineBounds::object_depth_max), the same bound the copy
//! is held to. An object nested past it is kept unread and named, and the file is refused only when a
//! page reaches that object or an in-place action rewrites every object; the reference has no cap and
//! fails on a file deeper than its stack holds. A body that numbers an object past
//! [`object_number_max`](crate::vocabulary::EngineBounds::object_number_max) is refused too: the writer
//! states one index entry per number up to the document's high-water mark, which at the top of the
//! range would wrap to nothing.

use std::collections::{BTreeMap, BTreeSet};

use lopdf::{Dictionary, Document, Object, ObjectId, Stream, StringFormat};

use crate::vocabulary::vocabulary;

/// Why a body scan refused a file.
#[derive(Debug)]
pub(super) enum Refusal {
    /// The bytes do not form a document this walk reads.
    Unreadable,
    /// An object nests deeper than the engine follows. [`Scan::indirect_objects`] keeps such an
    /// object unread and records it, and a trailer dictionary nested past the bound escapes
    /// [`document`] entirely — which [`super::page::open_with`] maps to `pdf_page_object_nested`.
    Nested,
    /// Recovering the document would read more bytes than the engine's bound allows.
    Bound,
    /// A container would decode to more bytes than the engine decodes. This is the engine's own
    /// ceiling rather than anything about the file, so it answers with a sentence of its own.
    Container,
    /// The file numbers an object past the highest number the writer holds.
    Number,
}

/// What a file's body states: the document the walk read, and what it could not read whole.
pub(super) struct Scanned {
    pub(super) document: Document,
    /// The objects [`Scan::indirect_objects`] kept unread because they nest past the engine's depth.
    pub(super) nested: BTreeSet<ObjectId>,
}

/// The bounds a scan is held to, every one of them stated in the vocabulary. A test injects its own
/// so a bound can be reached by a small fixture rather than by a file built to be large.
#[derive(Clone, Copy)]
pub(super) struct Bounds {
    /// How many times over its own length the scan may read a file, and an object stream's own bytes.
    pub(super) passes: usize,
    /// The deepest nesting of arrays and dictionaries the scan follows.
    pub(super) depth: usize,
    /// The bytes one look at what comes next may inspect.
    pub(super) look: usize,
    /// The bytes one container may decode to.
    pub(super) decode: usize,
    /// The highest object number the walk may find.
    pub(super) number: usize,
}

impl Bounds {
    /// The bounds the engine ships.
    pub(super) fn stated() -> Self {
        let engine = &vocabulary().engine;
        Self {
            passes: engine.pdf_scan_passes_max,
            depth: engine.object_depth_max,
            look: engine.scan_look_bytes_max,
            decode: engine.container_decode_max_bytes,
            number: engine.object_number_max,
        }
    }
}

/// The document the file's body states, read the way the reference reads one.
pub(super) fn document(bytes: &[u8], bounds: Bounds) -> Result<Scanned, Refusal> {
    let mut scan = Scan::new(bytes, bounds);
    let version = scan.header()?;
    // The reference's own loop: a section that consumed nothing would repeat for ever, so a stall is
    // a file it cannot read rather than one it retries.
    let mut previous = usize::MAX;
    while !scan.done() {
        scan.section()?;
        if scan.at == previous {
            return Err(Refusal::Unreadable);
        }
        previous = scan.at;
    }
    scan.recover_root();
    // The reference drops the object number the format reserves: nothing can reference `0 0`.
    scan.objects.remove(&(0, 0));
    let highest = scan
        .objects
        .keys()
        .map(|&(number, _)| number)
        .max()
        .unwrap_or(0);
    // The writer states one index entry per number up to this mark, so a file that numbers an object
    // past the engine's own bound is refused rather than written with an index it cannot hold.
    if !usize::try_from(highest).is_ok_and(|number| number <= bounds.number) {
        return Err(Refusal::Number);
    }
    let mut document = Document::with_version(version);
    document.max_id = highest;
    document.objects = scan.objects;
    document.trailer = scan.trailer;
    Ok(Scanned {
        document,
        nested: scan.nested,
    })
}

/// The bytes the scan may read: `passes` times `len`.
fn budget(len: usize, passes: usize) -> u64 {
    (len as u64).saturating_mul(passes as u64)
}

/// The scan's position in the file, the objects it has read and the trailer it has collected.
struct Scan<'a> {
    bytes: &'a [u8],
    at: usize,
    /// The bytes the scan may still read.
    left: u64,
    /// While a look at what comes next is in progress, the bytes it may still inspect; `None` while
    /// reading.
    looking: Option<u64>,
    /// The bounds the walk is held to.
    bounds: Bounds,
    objects: BTreeMap<ObjectId, Object>,
    /// The objects kept unread because they nest past the engine's depth.
    nested: BTreeSet<ObjectId>,
    /// Every object number in the order it was first defined. This is the only home of that order —
    /// the map is keyed by number — which is why root recovery walks it.
    order: Vec<ObjectId>,
    /// The four entries a trailer or an index stream states, the last statement of each winning.
    trailer: Dictionary,
}

impl<'a> Scan<'a> {
    fn new(bytes: &'a [u8], bounds: Bounds) -> Self {
        Self {
            bytes,
            at: 0,
            left: budget(bytes.len(), bounds.passes),
            looking: None,
            bounds,
            objects: BTreeMap::new(),
            nested: BTreeSet::new(),
            order: Vec::new(),
            trailer: Dictionary::new(),
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
            return Err(Refusal::Unreadable);
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
    /// section that consumes what the look found charges it from its own start.
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

    /// Charge `bytes` against the allowance in force: the file's while reading, a look's own while
    /// looking. Running out of a look's allowance is not a refusal — [`Self::look`] reads every error
    /// as "found nothing" — while running out of the file's is the engine's bound.
    fn charge(&mut self, bytes: u64) -> Result<(), Refusal> {
        if let Some(left) = &mut self.looking {
            *left = left.checked_sub(bytes).ok_or(Refusal::Unreadable)?;
            return Ok(());
        }
        self.left = self.left.checked_sub(bytes).ok_or(Refusal::Bound)?;
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
            return Err(Refusal::Unreadable);
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
        Err(Refusal::Unreadable)
    }

    /// The bytes after the header line are a binary mark, not an object header. The reference looks
    /// for a header there and, finding none, steps one byte past it.
    fn skip_binary_header_comment(&mut self) -> Result<(), Refusal> {
        self.skip_space_and_comments()?;
        if !self.look(|scan| scan.indirect_object_header().map(|_| true)) {
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
            match self.indirect_object() {
                Ok(()) => {}
                // The engine's own bounds are not a file it cannot read: they are a file it declines
                // to scan or decode, and they answer with their own sentences.
                Err(refusal @ (Refusal::Bound | Refusal::Container)) => return Err(refusal),
                Err(refusal) => {
                    self.seek(start);
                    let id = self.unreadable_object()?;
                    if matches!(refusal, Refusal::Nested) {
                        self.nested.insert(id);
                    }
                }
            }
            self.skip_space_and_comments()?;
            self.skip_jibberish()?;
        }
        Ok(())
    }

    fn indirect_object(&mut self) -> Result<(), Refusal> {
        let id = self.indirect_object_header()?;
        self.skip_space_and_comments()?;
        let object = self.object()?;
        self.skip_space_and_comments()?;
        let _ = self.keyword(b"endobj")?;
        if let Object::Stream(stream) = &object {
            match resolved_type(&stream.dict, &self.objects) {
                // A container the walk cannot expand is not carried: the writer would write the file
                // without what the container held — and, for an `/Encrypt` inside it, as an
                // unencrypted file. Every refusal propagates, so `indirect_objects` keeps the
                // container unread and the writer refuses the file.
                Some(kind) if kind == b"ObjStm" => return self.object_stream(stream),
                // The reference's parser never assigns an XRef stream, so neither does this walk.
                Some(kind) if kind == b"XRef" => return self.index_stream(stream),
                _ => {}
            }
        }
        self.assign(id, object);
        Ok(())
    }

    /// An object the walk cannot read: it is kept as a stream whose bytes were never read, and the
    /// writer refuses such a file rather than writing it without them. Answers the id it kept.
    fn unreadable_object(&mut self) -> Result<ObjectId, Refusal> {
        let id = self.indirect_object_header()?;
        self.skip_space_and_comments()?;
        let start = self.at;
        loop {
            if self.done() {
                return Err(Refusal::Unreadable);
            }
            if self.keyword(b"endobj")? {
                break;
            }
            self.next()?;
        }
        tracing::debug!(
            ?id,
            "document: an object could not be read and is kept unread"
        );
        self.assign(
            id,
            Object::Stream(Stream::with_position(Dictionary::new(), start)),
        );
        Ok(id)
    }

    fn indirect_object_header(&mut self) -> Result<ObjectId, Refusal> {
        self.skip_space_and_comments()?;
        let number = self.raw_int()?;
        self.skip_space_and_comments()?;
        let generation = self.raw_int()?;
        self.skip_space_and_comments()?;
        if !self.keyword(b"obj")? {
            return Err(Refusal::Unreadable);
        }
        let number = u32::try_from(number).map_err(|_| Refusal::Unreadable)?;
        let generation = u16::try_from(generation).map_err(|_| Refusal::Unreadable)?;
        Ok((number, generation))
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
        let dictionary = self.dictionary(0)?;
        self.adopt_trailer(&dictionary);
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
                        || scan.indirect_object_header().is_ok())
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
                self.trailer.set(key.to_vec(), value.clone());
            }
        }
    }

    /// Take the entries an index stream's dictionary states, replacing the trailer: an entry it
    /// leaves out is no longer stated at all.
    fn replace_trailer(&mut self, dictionary: &Dictionary) {
        for key in TRAILER_KEYS {
            match dictionary.get(key) {
                Ok(value) => self.trailer.set(key.to_vec(), value.clone()),
                Err(_) => {
                    self.trailer.remove(key);
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
            .cloned()
            .or_else(|| {
                let id = self.order.iter().rev().copied().find(|id| {
                    self.objects
                        .get(id)
                        .is_some_and(|object| is_catalog(object, &self.objects))
                })?;
                tracing::debug!(?id, "document: recovered the catalog from the body");
                let root = Object::Reference(id);
                self.trailer.set("Root", root.clone());
                Some(root)
            });
        let Some(Object::Dictionary(catalog)) = root else {
            return;
        };
        let Some(number) = self
            .objects
            .keys()
            .map(|&(number, _)| number)
            .max()
            .unwrap_or(0)
            .checked_add(1)
        else {
            return;
        };
        let id = (number, 0);
        tracing::debug!(?id, "document: gave an inline catalog a number of its own");
        self.assign(id, Object::Dictionary(catalog));
        self.trailer.set("Root", Object::Reference(id));
    }

    fn assign(&mut self, id: ObjectId, object: Object) {
        if !self.objects.contains_key(&id) {
            self.order.push(id);
        }
        // An object the walk has now read is no longer one it kept unread for its depth: a body that
        // nests too deep and is redefined with a body that does not parse is refused as the object it
        // could not read, not as one that nests.
        self.nested.remove(&id);
        self.objects.insert(id, object);
    }

    // ── the objects ─────────────────────────────────────────────────────────────────────────────

    fn object(&mut self) -> Result<Object, Refusal> {
        self.object_at(0)
    }

    /// An object `depth` arrays and dictionaries below the one being read. A file that nests deeper
    /// than the scan follows is not recursed into: the reference's parser has no cap and fails on
    /// such a file, and this engine must not exhaust its stack instead. [`Self::indirect_objects`]
    /// catches the refusal, keeps the object unread and records its id in `nested`, so the file is
    /// refused only when a page reaches that object or an in-place action rewrites every object.
    fn object_at(&mut self, depth: usize) -> Result<Object, Refusal> {
        if depth > self.bounds.depth {
            return Err(Refusal::Nested);
        }
        self.skip_space_and_comments()?;
        if self.keyword(b"true")? {
            return Ok(Object::Boolean(true));
        }
        if self.keyword(b"false")? {
            return Ok(Object::Boolean(false));
        }
        if self.keyword(b"null")? {
            return Ok(Object::Null);
        }
        match self.peek() {
            Some(b'<') if self.ahead(1) == Some(b'<') => self.dictionary_or_stream(depth),
            Some(b'<') => self.hexadecimal_string(),
            Some(b'(') => self.literal_string(),
            Some(b'/') => self.name().map(Object::Name),
            Some(b'[') => self.array(depth),
            Some(byte) if is_numeric(byte) => self.number_or_reference(),
            Some(_) | None => Err(Refusal::Unreadable),
        }
    }

    fn number_or_reference(&mut self) -> Result<Object, Refusal> {
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
                    return Err(Refusal::Unreadable);
                };
                let number = u32::try_from(*number).map_err(|_| Refusal::Unreadable)?;
                let generation = u16::try_from(*generation).map_err(|_| Refusal::Unreadable)?;
                return Ok(Object::Reference((number, generation)));
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
        let text =
            std::str::from_utf8(&self.bytes[start..self.at]).map_err(|_| Refusal::Unreadable)?;
        if text.is_empty() {
            return Err(Refusal::Unreadable);
        }
        if fractional {
            text.parse::<f32>()
                .map(Object::Real)
                .map_err(|_| Refusal::Unreadable)
        } else {
            text.parse::<i64>()
                .map(Object::Integer)
                .map_err(|_| Refusal::Unreadable)
        }
    }

    /// A whole number, as a section's counts and offsets state them. A run of digits longer than a
    /// `u64` holds is refused at the digit that would overflow it: the value cannot be one, and a
    /// look-ahead that read the whole run would spend work the bound never sees.
    fn raw_int(&mut self) -> Result<u64, Refusal> {
        let mut value = 0_u64;
        let mut digits = 0;
        while self.peek().is_some_and(is_digit) {
            let digit = u64::from(self.next()? - b'0');
            value = value
                .checked_mul(10)
                .and_then(|value| value.checked_add(digit))
                .ok_or(Refusal::Unreadable)?;
            digits += 1;
        }
        if digits == 0 {
            return Err(Refusal::Unreadable);
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

    fn array(&mut self, depth: usize) -> Result<Object, Refusal> {
        self.assert_next(b'[')?;
        self.skip_space_and_comments()?;
        let mut values = Vec::new();
        while self.peek() != Some(b']') {
            values.push(self.object_at(depth + 1)?);
            self.skip_space_and_comments()?;
        }
        self.next()?;
        Ok(Object::Array(values))
    }

    fn dictionary(&mut self, depth: usize) -> Result<Dictionary, Refusal> {
        if !self.keyword(b"<<")? {
            return Err(Refusal::Unreadable);
        }
        self.skip_space_and_comments()?;
        let mut dictionary = Dictionary::new();
        while !self.done() && self.peek() != Some(b'>') {
            let key = self.name()?;
            let value = self.object_at(depth + 1)?;
            dictionary.set(key, value);
            self.skip_space_and_comments()?;
        }
        self.skip_space_and_comments()?;
        if !self.keyword(b">>")? {
            return Err(Refusal::Unreadable);
        }
        Ok(dictionary)
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
        Err(Refusal::Unreadable)
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
        Err(Refusal::Unreadable)
    }

    /// One escape sequence of a literal string, written into `bytes` the way the object model reads
    /// it: an octal code, a control character's letter, a line continuation, or the character itself.
    fn escape(&mut self, bytes: &mut Vec<u8>) -> Result<(), Refusal> {
        let Some(byte) = self.peek() else {
            return Err(Refusal::Unreadable);
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

    fn dictionary_or_stream(&mut self, depth: usize) -> Result<Object, Refusal> {
        let dictionary = self.dictionary(depth)?;
        self.skip_space_and_comments()?;
        // The keyword's own end of line is part of it, whichever spelling it takes.
        if !(self.keyword(b"stream \r\n")?
            || self.keyword(b"stream\r\n")?
            || self.keyword(b"stream\r")?
            || self.keyword(b"stream\n")?
            || self.keyword(b"stream")?)
        {
            return Ok(Object::Dictionary(dictionary));
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
        let contents = self.bytes[start.min(end)..end].to_vec();
        Ok(Object::Stream(Stream::new(dictionary, contents)))
    }

    /// The end of a stream's bytes, found by scanning for its own end marker: nested `stream`
    /// keywords are counted, so a file whose bytes contain one is read the way the reference reads
    /// it rather than ending early.
    fn end_of_stream(&mut self) -> Result<usize, Refusal> {
        let mut nesting = 1_i64;
        while !self.done() {
            let end = self.at;
            if self.keyword(b"stream")? {
                nesting += 1;
            } else if self.keyword(b"\r\nendstream")?
                || self.keyword(b"\rendstream")?
                || self.keyword(b"\nendstream")?
                || self.keyword(b"endstream")?
            {
                nesting -= 1;
            } else {
                self.next()?;
            }
            if nesting == 0 {
                return Ok(end);
            }
        }
        Err(Refusal::Unreadable)
    }

    /// The objects an object stream holds, read from its decoded bytes. The container's filter chain
    /// is [`decoded`], which reads the filters the reference reads.
    fn object_stream(&mut self, stream: &Stream) -> Result<(), Refusal> {
        let first = stream.dict.get(b"First").map_err(|_| Refusal::Unreadable)?;
        let first = whole(
            resolved(first, &self.objects)
                .as_float()
                .map_err(|_| Refusal::Unreadable)?,
        );
        let count = stream.dict.get(b"N").map_err(|_| Refusal::Unreadable)?;
        let count = whole(
            resolved(count, &self.objects)
                .as_float()
                .map_err(|_| Refusal::Unreadable)?,
        );
        let data =
            decoded(stream, &self.objects, self.bounds.decode).map_err(|decode| match decode {
                Decode::Undecodable => Refusal::Unreadable,
                Decode::TooLarge => Refusal::Container,
            })?;
        // The container's bytes are bounded by their own decoded length, not by the file's: a modern
        // file compresses far better than the scan bound allows for, so a share of the file's budget
        // would refuse a container the reference reads.
        let mut nested = Scan::new(&data, self.bounds);
        // Each entry states two numbers, so a container holds at most one entry per four bytes of its
        // own decoded length. A count past that is a container that does not hold what it says; the
        // bound keeps the entries built from it at no more than twice the bytes they were read from.
        if count > data.len() / 4 {
            return Err(Refusal::Unreadable);
        }
        let mut entries = Vec::new();
        for _ in 0..count {
            nested.skip_space_and_comments()?;
            let number = u32::try_from(nested.raw_int()?).map_err(|_| Refusal::Unreadable)?;
            nested.skip_space_and_comments()?;
            let offset = u32::try_from(nested.raw_int()?).map_err(|_| Refusal::Unreadable)?;
            entries.push((number, offset));
        }
        for (number, offset) in entries {
            // A host that cannot hold an offset this container states seeks past the container, where
            // the walk finds nothing.
            let offset = usize::try_from(offset).unwrap_or(usize::MAX);
            nested.seek(first.saturating_add(offset).min(nested.bytes.len()));
            let object = nested.object()?;
            nested.assign((number, 0), object);
        }
        tracing::debug!(objects = count, "document: expanded an object stream");
        for id in std::mem::take(&mut nested.order) {
            if let Some(object) = nested.objects.remove(&id) {
                self.assign(id, object);
            }
        }
        Ok(())
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
    /// The decode is the reference's own eager one, so its bytes are held once and dropped; a container
    /// past the engine's ceiling is refused rather than decoded, because the entries it states — an
    /// `/Encrypt` among them — are not something to lose silently.
    fn index_stream(&mut self, stream: &Stream) -> Result<(), Refusal> {
        match decoded(stream, &self.objects, self.bounds.decode) {
            Ok(_) => {}
            Err(Decode::Undecodable) => return Ok(()),
            Err(Decode::TooLarge) => return Err(Refusal::Container),
        }
        let dictionary = &stream.dict;
        let Ok(size) = dictionary.get(b"Size") else {
            return Ok(());
        };
        if resolved(size, &self.objects).as_float().is_err() {
            return Ok(());
        }
        let Ok(widths) = dictionary.get(b"W") else {
            return Ok(());
        };
        let Ok(widths) = resolved(widths, &self.objects).as_array() else {
            return Ok(());
        };
        if !widths
            .iter()
            .all(|width| resolved(width, &self.objects).as_float().is_ok())
        {
            return Ok(());
        }
        // The reference falls back to `[0 Size]` when `/Index` is not an array, and throws only on an
        // element that is not a number, so a `/Index` of another shape is no statement at all.
        if let Ok(index) = dictionary.get(b"Index")
            && let Ok(index) = resolved(index, &self.objects).as_array()
            && (index.len() % 2 != 0
                || !index
                    .iter()
                    .all(|value| resolved(value, &self.objects).as_float().is_ok()))
        {
            return Ok(());
        }
        self.replace_trailer(dictionary);
        Ok(())
    }
}

/// The entries a `trailer` dictionary or an index stream's dictionary states.
const TRAILER_KEYS: &[&[u8]] = &[b"Root", b"Encrypt", b"Info", b"ID"];

/// Whether `object` is a catalog: a dictionary whose `/Type` is `Catalog`, followed through a
/// reference on the object and on its `/Type` alike.
fn is_catalog(object: &Object, objects: &BTreeMap<ObjectId, Object>) -> bool {
    match resolved(object, objects) {
        Object::Dictionary(dictionary) => resolved_type(dictionary, objects) == Some(b"Catalog"),
        _ => false,
    }
}

/// `object`, followed once through the object map when it is a reference.
fn resolved<'a>(object: &'a Object, objects: &'a BTreeMap<ObjectId, Object>) -> &'a Object {
    match object {
        Object::Reference(id) => objects.get(id).unwrap_or(object),
        _ => object,
    }
}

/// A dictionary's `/Type`, resolved through the object map.
fn resolved_type<'a>(
    dictionary: &'a Dictionary,
    objects: &'a BTreeMap<ObjectId, Object>,
) -> Option<&'a [u8]> {
    let value = dictionary.get(b"Type").ok()?;
    resolved(value, objects).as_name().ok()
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

fn is_numeric(byte: u8) -> bool {
    byte.is_ascii_digit() || matches!(byte, b'+' | b'-' | b'.')
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

/// Why a container's bytes were not decoded.
#[derive(Debug)]
enum Decode {
    /// A filter chain or bytes this engine does not decode.
    Undecodable,
    /// The stream would decode past the engine's ceiling.
    TooLarge,
}

/// The stream's bytes through its filter chain. lopdf reads Flate, LZW and ASCII85; the reference's
/// library also reads ASCIIHex and RunLength, so those two are decoded here.
///
/// `/Filter` and `/DecodeParms` are resolved through the object map, as the reference resolves them,
/// so a chain stated indirectly is the chain it names. `max` is
/// [`container_decode_max_bytes`](crate::vocabulary::EngineBounds::container_decode_max_bytes): the
/// decoders written here check a run before they allocate it, while a filter lopdf decodes states its
/// output only once it has allocated it. For those the ceiling is applied to the result: a stream
/// crafted to expand past it allocates its whole output — up to its filter's expansion ratio over the
/// stream's own bytes — before the length is checked, and an allocation failure aborts the process
/// rather than unwinding. The decoders are trusted, not hardened, as they are in the reading half.
fn decoded(
    stream: &Stream,
    objects: &BTreeMap<ObjectId, Object>,
    max: usize,
) -> Result<Vec<u8>, Decode> {
    if stream.content.len() > max {
        return Err(Decode::TooLarge);
    }
    let filters = filters(stream, objects).ok_or(Decode::Undecodable)?;
    let mut data = stream.content.clone();
    for (filter, parms) in filters {
        data = match filter.as_slice() {
            b"ASCIIHexDecode" => ascii_hex(&data),
            b"RunLengthDecode" => run_length(&data, max)?,
            _ => {
                // One filter at a time, over the bytes the one before left: the dictionary is cloned
                // for the single-filter stream and the content is not.
                let mut single = Stream::new(stream.dict.clone(), data);
                single.dict.set("Filter", Object::Name(filter));
                match parms {
                    Some(parms) => single.dict.set("DecodeParms", Object::Dictionary(parms)),
                    None => {
                        single.dict.remove(b"DecodeParms");
                    }
                }
                single
                    .decompressed_content()
                    .map_err(|_| Decode::Undecodable)?
            }
        };
        // A decoder that is not ours states its output only once it has allocated it: the total is
        // checked here, before it is handed on.
        if data.len() > max {
            return Err(Decode::TooLarge);
        }
    }
    Ok(data)
}

/// A stream's filter chain, each filter with the decode parameters stated for it. `/Filter` and
/// `/DecodeParms` are resolved through the object map, because the reference reads them with
/// `lookup`: a `/Filter` that names an object is the name, or the array of names, it names.
///
/// `None` is a chain this engine does not read: a `/Filter` that resolves to neither a name nor an
/// array of names, or an array whose element is not a name.
fn filters(
    stream: &Stream,
    objects: &BTreeMap<ObjectId, Object>,
) -> Option<Vec<(Vec<u8>, Option<Dictionary>)>> {
    let Ok(filter) = stream.dict.get(b"Filter") else {
        return Some(Vec::new());
    };
    let parms = stream
        .dict
        .get(b"DecodeParms")
        .ok()
        .map(|parms| resolved(parms, objects));
    match resolved(filter, objects) {
        // A chain of one reads the parameters as they stand, so an array states none.
        Object::Name(name) => {
            let parms = parms.and_then(|parms| parms.as_dict().ok().cloned());
            Some(vec![(name.clone(), parms)])
        }
        // A chain of several reads each filter's own entry, so a single dictionary states none.
        Object::Array(names) => names
            .iter()
            .enumerate()
            .map(|(index, name)| {
                let name = resolved(name, objects).as_name().ok()?.to_vec();
                let parms = match parms {
                    Some(Object::Array(values)) => values.get(index),
                    _ => None,
                }
                .and_then(|parms| resolved(parms, objects).as_dict().ok().cloned());
                Some((name, parms))
            })
            .collect(),
        _ => None,
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

/// RunLengthDecode: a length byte `n`; `0..=127` copies the next `n + 1` bytes, `129..=255` repeats
/// the next byte `257 - n` times, and `0x80` ends the data. Bytes running out before the end marker is
/// a refusal, and so is a run that would take the output past `max` bytes: two input bytes can state
/// a run of 128, so the check is made before the run is allocated and a stream built to expand without
/// end costs no more than the bound. The reference ends a truncated run at the end of the bytes; this
/// engine refuses it, since a container that lost what it held is not one to carry.
fn run_length(data: &[u8], max: usize) -> Result<Vec<u8>, Decode> {
    let mut out = Vec::new();
    let mut index = 0;
    loop {
        let &length = data.get(index).ok_or(Decode::Undecodable)?;
        index += 1;
        match length {
            0x80 => return Ok(out),
            0..=127 => {
                let count = usize::from(length) + 1;
                let chunk = data.get(index..index + count).ok_or(Decode::Undecodable)?;
                if out.len() + count > max {
                    return Err(Decode::TooLarge);
                }
                out.extend_from_slice(chunk);
                index += count;
            }
            _ => {
                let &byte = data.get(index).ok_or(Decode::Undecodable)?;
                index += 1;
                let count = 257 - usize::from(length);
                if out.len() + count > max {
                    return Err(Decode::TooLarge);
                }
                out.extend(std::iter::repeat_n(byte, count));
            }
        }
    }
}

/// A count or an offset the format states as a number, as a whole one.
#[expect(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "a stream's first offset and object count are small whole numbers"
)]
fn whole(value: f32) -> usize {
    value as usize
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn an_object_stream_is_bounded_by_its_own_bytes() {
        // A container holds far more than the file that states it, so the nested walk gets its own
        // allowance: a parent with nothing left still reads the objects the container holds.
        let mut scan = Scan::new(b"", Bounds::stated());
        scan.object_stream(&container())
            .expect("the container is read");
        assert_eq!(scan.objects.len(), 2);
    }

    #[test]
    fn an_object_deeper_than_the_scan_follows_is_refused() {
        // The reference's parser fails on a file deeper than its stack holds, and this engine must
        // refuse it rather than exhaust its own stack.
        let mut shallow = Scan::new(
            b"[[[[1]]]]",
            Bounds {
                depth: 4,
                ..Bounds::stated()
            },
        );
        assert!(shallow.object().is_ok());
        let mut deep = Scan::new(
            b"[[[[[1]]]]]",
            Bounds {
                depth: 4,
                ..Bounds::stated()
            },
        );
        assert!(matches!(deep.object(), Err(Refusal::Nested)));
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
        scan.index_stream(&stream).expect("nothing is adopted");
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
            scan.index_stream(&Stream::new(dict, Vec::new()))
                .expect("nothing is adopted");
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
    fn a_truncated_run_length_stream_is_refused() {
        // A deliberate divergence: the reference ends a truncated run at the end of the bytes, while a
        // container that lost what it held is not one to carry — so the file is refused instead.
        let mut dict = Dictionary::new();
        dict.set("Filter", Object::Name(b"RunLengthDecode".to_vec()));
        let stream = Stream::new(dict, vec![5, b'a']);
        assert!(matches!(
            decoded(&stream, &BTreeMap::new(), Bounds::stated().decode),
            Err(Decode::Undecodable)
        ));
    }

    #[test]
    fn a_run_length_stream_that_would_decode_past_the_bound_is_refused() {
        // Two input bytes state a run of 128, so a modest file can state a stream that expands
        // without end. The runs are checked as they are read, before one is allocated — the ceiling
        // here is a test's, so the proof costs a handful of bytes rather than the shipped ceiling.
        let mut dict = Dictionary::new();
        dict.set("Filter", Object::Name(b"RunLengthDecode".to_vec()));
        // Nine runs of 128, each stated by two bytes, against a ceiling of a thousand.
        let mut data = Vec::new();
        for _ in 0..=8 {
            data.extend_from_slice(&[0x81, b'z']);
        }
        data.push(0x80);
        let stream = Stream::new(dict, data);
        assert!(matches!(
            decoded(&stream, &BTreeMap::new(), 1000),
            Err(Decode::TooLarge)
        ));
    }

    #[test]
    fn a_container_past_the_engine_ceiling_is_refused_with_the_engines_own_refusal() {
        // The ceiling is the engine's, not a shape of the file: a container past it answers as the
        // engine declining to decode, so the file is refused rather than reported unreadable.
        let mut scan = Scan::new(
            b"",
            Bounds {
                decode: 4,
                ..Bounds::stated()
            },
        );
        assert!(matches!(
            scan.object_stream(&container()),
            Err(Refusal::Container)
        ));
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
        scan.index_stream(&Stream::new(dict, vec![1, 2]))
            .expect("the entries are adopted");
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
        scan.index_stream(&Stream::new(dict, Vec::new()))
            .expect("the entries are adopted");
        assert_eq!(
            scan.trailer.get(b"Encrypt").ok(),
            Some(&Object::Reference((2, 0)))
        );
    }

    #[test]
    fn a_body_read_later_is_no_longer_one_kept_for_its_depth() {
        // An object first defined too deep and then redefined with a body that parses is an object
        // the walk read: it is not reported as nesting too deep.
        let bytes = b"1 0 obj\n[[[[[1]]]]]\nendobj\n1 0 obj\n7\nendobj\n";
        let mut scan = Scan::new(
            bytes,
            Bounds {
                depth: 4,
                ..Bounds::stated()
            },
        );
        scan.indirect_objects().expect("the objects are read");
        assert!(scan.nested.is_empty());
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
        scan.assign((1, 0), Object::Integer(2));
        scan.assign((2, 0), Object::Integer(8));
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
    fn a_number_too_long_for_a_u64_is_refused_at_the_digit_that_overflows_it() {
        // A look-ahead parses numbers without charging the file, so a run of digits longer than a
        // `u64` holds stops at the digit that overflows rather than being read whole at every look.
        let mut scan = Scan::new(b"99999999999999999999999", Bounds::stated());
        assert!(matches!(scan.raw_int(), Err(Refusal::Unreadable)));
        assert_eq!(scan.at, 20, "the digits past the overflow were read");
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
        scan.assign((1, 0), Object::Name(b"ObjStm".to_vec()));
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
            let Object::Stream(stream) = scan.object().expect("a stream") else {
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
        let Object::String(bytes, format) = scan.object().expect("a string") else {
            panic!("a string was not read as one");
        };
        assert_eq!(bytes, vec![0x41, 0x40]);
        assert_eq!(format, StringFormat::Hexadecimal);
    }
}

//! The legacy binary formats' corpus documents, assembled byte by byte.
//!
//! A `.doc`, `.xls` or `.ppt` is a compound file (CFB) container around hand-built Word, BIFF and
//! PowerPoint streams. This module assembles them, so the legacy arm of the reader has a generated
//! corpus like every other family and no fixture file is committed.

#![expect(
    clippy::cast_possible_truncation,
    reason = "the fixtures' stream offsets and counts fit the CFB/BIFF/PPT record fields by construction"
)]

const END_OF_CHAIN: u32 = 0xFFFF_FFFE;
const FAT_SECT: u32 = 0xFFFF_FFFD;
const FREE_SECT: u32 = 0xFFFF_FFFF;
const NO_ENTRY: u32 = 0xFFFF_FFFF;

/// Assemble a minimal v3 CFB from root-level streams, however many directory entries it takes,
/// chained through the FAT like every other chain. The root entry declares no stream of its own, so
/// every stream is read from its regular sectors.
fn cfb(streams: &[(&str, &[u8])]) -> Vec<u8> {
    let sized: Vec<(&str, &[u8], u32)> = streams
        .iter()
        .map(|(name, data)| (*name, *data, data.len() as u32))
        .collect();
    cfb_sized(&sized)
}

/// [`cfb`] with each stream's directory entry declaring `size` bytes rather than the bytes it
/// really holds: a stream declared longer than its chain is what a reader reports as cut short.
fn cfb_sized(streams: &[(&str, &[u8], u32)]) -> Vec<u8> {
    let dir_sectors = (streams.len() + 1).div_ceil(4) as u32;
    let fat_sector = dir_sectors;
    let mut starts = Vec::new();
    let mut runs = Vec::new();
    let mut next = fat_sector + 1;
    for (_, data, _) in streams {
        let sectors = data.len().div_ceil(512).max(1) as u32;
        starts.push(next);
        runs.push(sectors);
        next += sectors;
    }
    let total_sectors = next as usize;
    assert!(total_sectors <= 128, "one FAT sector covers 128 sectors");

    let mut file = vec![0u8; 512 * (1 + total_sectors)];
    file[0..8].copy_from_slice(&[0xD0, 0xCF, 0x11, 0xE0, 0xA1, 0xB1, 0x1A, 0xE1]);
    file[0x18..0x1A].copy_from_slice(&0x003Eu16.to_le_bytes());
    file[0x1A..0x1C].copy_from_slice(&3u16.to_le_bytes());
    file[0x1C..0x1E].copy_from_slice(&0xFFFEu16.to_le_bytes());
    file[0x1E..0x20].copy_from_slice(&9u16.to_le_bytes());
    file[0x20..0x22].copy_from_slice(&6u16.to_le_bytes());
    file[0x2C..0x30].copy_from_slice(&1u32.to_le_bytes()); // one FAT sector
    file[0x30..0x34].copy_from_slice(&0u32.to_le_bytes()); // directory at sector 0
    file[0x38..0x3C].copy_from_slice(&4096u32.to_le_bytes());
    file[0x3C..0x40].copy_from_slice(&END_OF_CHAIN.to_le_bytes());
    file[0x44..0x48].copy_from_slice(&END_OF_CHAIN.to_le_bytes());
    file[0x4C..0x50].copy_from_slice(&fat_sector.to_le_bytes()); // DIFAT[0] = FAT sector
    for i in 1..109 {
        let off = 0x4C + i * 4;
        file[off..off + 4].copy_from_slice(&FREE_SECT.to_le_bytes());
    }

    let dir = 512;
    let root_child = if streams.is_empty() { NO_ENTRY } else { 1 };
    write_entry(
        &mut file[dir..dir + 128],
        "Root Entry",
        5,
        root_child,
        END_OF_CHAIN,
        0,
    );
    for (index, (name, data, size)) in streams.iter().enumerate() {
        let right = if index + 1 < streams.len() {
            (index as u32) + 2
        } else {
            NO_ENTRY
        };
        let at = dir + (index + 1) * 128;
        write_entry(
            &mut file[at..at + 128],
            name,
            2,
            NO_ENTRY,
            starts[index],
            *size,
        );
        file[at + 0x48..at + 0x4C].copy_from_slice(&right.to_le_bytes());
        let offset = 512 * (starts[index] as usize + 1);
        file[offset..offset + data.len()].copy_from_slice(data);
    }

    let fat = 512 * (fat_sector as usize + 1);
    let mut entries = vec![FREE_SECT; 128];
    for k in 0..dir_sectors {
        entries[k as usize] = if k + 1 == dir_sectors {
            END_OF_CHAIN
        } else {
            k + 1
        };
    }
    entries[fat_sector as usize] = FAT_SECT;
    for (index, &start) in starts.iter().enumerate() {
        for k in 0..runs[index] {
            entries[(start + k) as usize] = if k + 1 == runs[index] {
                END_OF_CHAIN
            } else {
                start + k + 1
            };
        }
    }
    for (index, value) in entries.iter().enumerate() {
        file[fat + index * 4..fat + index * 4 + 4].copy_from_slice(&value.to_le_bytes());
    }
    file
}

fn write_entry(buf: &mut [u8], name: &str, kind: u8, child: u32, start: u32, size: u32) {
    for (i, unit) in name.encode_utf16().enumerate() {
        buf[i * 2..i * 2 + 2].copy_from_slice(&unit.to_le_bytes());
    }
    let name_size = ((name.encode_utf16().count() + 1) * 2) as u16;
    buf[0x40..0x42].copy_from_slice(&name_size.to_le_bytes());
    buf[0x42] = kind;
    buf[0x43] = 1;
    buf[0x44..0x48].copy_from_slice(&NO_ENTRY.to_le_bytes());
    buf[0x48..0x4C].copy_from_slice(&NO_ENTRY.to_le_bytes());
    buf[0x4C..0x50].copy_from_slice(&child.to_le_bytes());
    buf[0x74..0x78].copy_from_slice(&start.to_le_bytes());
    buf[0x78..0x7C].copy_from_slice(&size.to_le_bytes());
}

/// One subdocument story a `.doc` can carry, with its `FibRgLw97` slot in the fixed `[MS-DOC]`
/// order the reader walks (`text` is slot 0; slot 3 is the ignored macro length).
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Story {
    Footnote,
    Header,
    Comment,
    Endnote,
}

impl Story {
    const fn slot(self) -> usize {
        match self {
            Self::Footnote => 1,
            Self::Header => 2,
            Self::Comment => 4,
            Self::Endnote => 5,
        }
    }
}

/// A `.doc` whose main text is `body` and whose one footnote is `footnote`.
pub fn doc(body: &str, footnote: &str) -> Vec<u8> {
    doc_stories(body, &[(Story::Footnote, footnote)])
}

/// A `.doc` whose main text is `body` and whose `stories` are its subdocuments, each laid out as
/// one UTF-16LE piece in the fixed story order.
pub fn doc_stories(body: &str, stories: &[(Story, &str)]) -> Vec<u8> {
    const TEXT_FC: u32 = 0x400;
    let mut word = vec![0u8; TEXT_FC as usize];
    let mut ccp = [0u32; 8];
    let mut pieces = Vec::new();
    let append = |word: &mut Vec<u8>, text: &str| {
        let fc = word.len() as u32;
        let count = text.encode_utf16().count() as u32;
        word.extend(text.encode_utf16().flat_map(u16::to_le_bytes));
        (fc, count)
    };
    let (fc, count) = append(&mut word, body);
    ccp[0] = count;
    pieces.push((fc, count));
    let mut ordered: Vec<(usize, &str)> = stories
        .iter()
        .map(|(story, text)| (story.slot(), *text))
        .collect();
    ordered.sort_unstable_by_key(|(slot, _)| *slot);
    for (slot, text) in ordered {
        let (fc, count) = append(&mut word, text);
        ccp[slot] = count;
        if count > 0 {
            pieces.push((fc, count));
        }
    }
    let clx = doc_clx(&pieces);
    word[..1024].copy_from_slice(&doc_fib(&ccp, 0, clx.len() as u32));
    cfb(&[("WordDocument", &word), ("1Table", &clx)])
}

/// A `.doc` whose `WordDocument` stream declares more bytes than it holds: the reader's piece
/// table still yields the text, and the loss is named.
pub fn doc_cut(body: &str) -> Vec<u8> {
    let whole = doc(body, "");
    let (word, clx) = cfb_streams(&whole);
    cfb_sized(&[
        ("WordDocument", &word, word.len() as u32 + 512),
        ("1Table", &clx, clx.len() as u32),
    ])
}

/// The two streams of a `.doc` this module built: the `WordDocument` stream and its `1Table`.
fn cfb_streams(package: &[u8]) -> (Vec<u8>, Vec<u8>) {
    let mut reader = office_oxide::cfb::CfbReader::new(std::io::Cursor::new(package))
        .expect("a document this module built is a CFB container");
    let word = reader
        .open_stream("WordDocument")
        .expect("the WordDocument stream it wrote");
    let table = reader
        .open_stream("1Table")
        .expect("the 1Table stream it wrote");
    (word, table)
}

/// A CFB container holding the two streams a genuinely encrypted OOXML package carries and
/// nothing readable.
pub fn encrypted_package() -> Vec<u8> {
    cfb(&[
        ("EncryptionInfo", b"encryption info bytes"),
        ("EncryptedPackage", b"encrypted package bytes"),
    ])
}

/// A Word 6.0/95 FIB ([MS-DOC]'s predecessor) with `wIdent` 0xA5DC and no text: the version a
/// reader may refuse rather than call corrupt.
pub fn doc_old_version() -> Vec<u8> {
    let mut word = vec![0u8; 1024];
    word[0..2].copy_from_slice(&0xA5DCu16.to_le_bytes());
    word[2..4].copy_from_slice(&0x0065u16.to_le_bytes());
    cfb(&[("WordDocument", &word), ("1Table", &[])])
}

/// A CFB whose `WordDocument` stream opens with an identifier no Word version uses: a container
/// that is a `.doc` by its name alone, which the reader calls corrupt.
pub fn doc_corrupt() -> Vec<u8> {
    let mut word = vec![0u8; 1024];
    word[0..2].copy_from_slice(&0x1234u16.to_le_bytes());
    cfb(&[("WordDocument", &word), ("1Table", &[])])
}

/// A Word 97 FIB ([MS-DOC] §2.5.1): `wIdent` 0xA5EC, the 1Table flag, the `ccp*` lengths and the
/// CLX pointer. `ccp` is the eight `FibRgLw97` lengths in `[MS-DOC]` order.
fn doc_fib(ccp: &[u32; 8], fc_clx: u32, lcb_clx: u32) -> Vec<u8> {
    let mut fib = vec![0u8; 1024];
    fib[0..2].copy_from_slice(&0xA5ECu16.to_le_bytes());
    fib[2..4].copy_from_slice(&0x00C1u16.to_le_bytes());
    fib[0x0A..0x0C].copy_from_slice(&(1u16 << 9).to_le_bytes());
    for (index, length) in ccp.iter().enumerate() {
        let at = 0x4C + index * 4;
        fib[at..at + 4].copy_from_slice(&length.to_le_bytes());
    }
    fib[0x01A2..0x01A6].copy_from_slice(&fc_clx.to_le_bytes());
    fib[0x01A6..0x01AA].copy_from_slice(&lcb_clx.to_le_bytes());
    fib
}

/// A CLX whose Pcdt holds one Unicode piece per `(fc, char_count)` in order, the CP ranges running
/// consecutively from 0.
fn doc_clx(pieces: &[(u32, u32)]) -> Vec<u8> {
    let mut cps = Vec::new();
    let mut pcds = Vec::new();
    let mut cp = 0u32;
    for (fc, char_count) in pieces {
        cps.extend_from_slice(&cp.to_le_bytes());
        pcds.extend_from_slice(&0u16.to_le_bytes()); // PCD unused
        pcds.extend_from_slice(&fc.to_le_bytes()); // Unicode: fc used directly
        pcds.extend_from_slice(&0u16.to_le_bytes()); // prm
        cp += char_count;
    }
    cps.extend_from_slice(&cp.to_le_bytes());
    cps.extend_from_slice(&pcds);
    let mut clx = vec![0x02u8]; // Pcdt
    clx.extend_from_slice(&(cps.len() as u32).to_le_bytes());
    clx.extend_from_slice(&cps);
    clx
}

const RT_BOF: u16 = 0x0809;
const RT_EOF: u16 = 0x000A;
const RT_BOUNDSHEET: u16 = 0x0085;
const RT_SST: u16 = 0x00FC;
const RT_LABELSST: u16 = 0x00FD;
const RT_NUMBER: u16 = 0x0203;
/// One chart series' name, the chart text a legacy reading does not print.
const RT_SERIESTEXT: u16 = 0x100D;

/// The Workbook stream of a one-sheet `.xls`: cell A1 the shared string `text` and B2 the number
/// 3.5.
fn xls_stream(text: &str) -> Vec<u8> {
    let mut stream = bof(0x0005); // globals
    stream.extend(boundsheet("Данные"));
    stream.extend(sst(text));
    stream.extend(biff_rec(RT_EOF, &[]));
    stream.extend(bof(0x0010)); // the sheet's own substream
    stream.extend(labelsst(0, 0));
    stream.extend(number(1, 1, 3.5));
    stream.extend(biff_rec(RT_EOF, &[]));
    stream
}

/// A `.xls` holding one sheet, `Данные`, with A1 the shared string `text` and B2 the number 3.5.
pub fn xls(text: &str) -> Vec<u8> {
    cfb(&[("Workbook", &xls_stream(text))])
}

/// A `.xls` whose `Workbook` stream declares more bytes than it holds: the cells it reached are
/// still read and the loss is named.
pub fn xls_cut(text: &str) -> Vec<u8> {
    let stream = xls_stream(text);
    cfb_sized(&[("Workbook", &stream, stream.len() as u32 + 512)])
}

/// A CFB container with no `Workbook` stream, named `.xls`: the container is a spreadsheet by its
/// name alone, so the reader gives that family's corrupt verdict.
pub fn xls_corrupt() -> Vec<u8> {
    cfb(&[("Contents", b"not a workbook stream")])
}

/// A `.xls` whose second sheet is a chart sheet: its substream holds one `SeriesText`, the chart
/// text the reading does not print.
pub fn xls_chart_sheet(text: &str) -> Vec<u8> {
    let mut stream = bof(0x0005); // globals
    stream.extend(boundsheet("Данные"));
    stream.extend(boundsheet("Chart1"));
    stream.extend(sst(text));
    stream.extend(biff_rec(RT_EOF, &[]));
    stream.extend(bof(0x0010)); // the sheet's own substream
    stream.extend(labelsst(0, 0));
    stream.extend(biff_rec(RT_EOF, &[]));
    stream.extend(bof(0x0020)); // the chart sheet's own substream
    stream.extend(series_text("Revenue"));
    stream.extend(biff_rec(RT_EOF, &[]));
    cfb(&[("Workbook", &stream)])
}

/// One `SeriesText` record: two reserved bytes, the character count, the flags byte and the
/// characters — a chart's series name, which a legacy reading does not print.
fn series_text(text: &str) -> Vec<u8> {
    let mut data = vec![0u8, 0u8];
    data.push(text.chars().count() as u8);
    data.push(0);
    data.extend_from_slice(text.as_bytes());
    biff_rec(RT_SERIESTEXT, &data)
}

fn biff_rec(rec_type: u16, data: &[u8]) -> Vec<u8> {
    let mut buf = rec_type.to_le_bytes().to_vec();
    buf.extend_from_slice(&(data.len() as u16).to_le_bytes());
    buf.extend_from_slice(data);
    buf
}

fn bof(doctype: u16) -> Vec<u8> {
    let mut body = 0x0600u16.to_le_bytes().to_vec();
    body.extend_from_slice(&doctype.to_le_bytes());
    body.extend_from_slice(&[0u8; 12]);
    biff_rec(RT_BOF, &body)
}

/// A `BOUNDSHEET` with a wide (UTF-16LE) name, so a non-Latin sheet name exercises the Unicode
/// name path.
fn boundsheet(name: &str) -> Vec<u8> {
    let mut body = Vec::new();
    body.extend_from_slice(&0u32.to_le_bytes()); // stream offset, unused
    body.push(0); // visible
    body.push(0); // worksheet
    body.push(name.encode_utf16().count() as u8);
    body.push(0x01); // wide
    for unit in name.encode_utf16() {
        body.extend_from_slice(&unit.to_le_bytes());
    }
    biff_rec(RT_BOUNDSHEET, &body)
}

/// An SST holding one wide string, so the Cyrillic cell pins the Unicode string path.
fn sst(text: &str) -> Vec<u8> {
    let mut body = 1u32.to_le_bytes().to_vec(); // total
    body.extend_from_slice(&1u32.to_le_bytes()); // unique
    body.extend_from_slice(&(text.encode_utf16().count() as u16).to_le_bytes());
    body.push(0x01); // wide
    for unit in text.encode_utf16() {
        body.extend_from_slice(&unit.to_le_bytes());
    }
    biff_rec(RT_SST, &body)
}

fn labelsst(row: u16, col: u16) -> Vec<u8> {
    let mut body = row.to_le_bytes().to_vec();
    body.extend_from_slice(&col.to_le_bytes());
    body.extend_from_slice(&0u16.to_le_bytes()); // xf
    body.extend_from_slice(&0u32.to_le_bytes()); // shared string index
    biff_rec(RT_LABELSST, &body)
}

fn number(row: u16, col: u16, value: f64) -> Vec<u8> {
    let mut body = row.to_le_bytes().to_vec();
    body.extend_from_slice(&col.to_le_bytes());
    body.extend_from_slice(&0u16.to_le_bytes()); // xf
    body.extend_from_slice(&value.to_le_bytes());
    biff_rec(RT_NUMBER, &body)
}

const RT_DOCUMENT: u16 = 0x03E8;
const RT_SLIDE: u16 = 0x03EE;
const RT_SLIDE_ATOM: u16 = 0x03EF;
const RT_NOTES: u16 = 0x03F0;
const RT_NOTES_ATOM: u16 = 0x03F1;
const RT_SLIDE_PERSIST_ATOM: u16 = 0x03F3;
const RT_SLIDE_LIST_WITH_TEXT: u16 = 0x0FF0;
const RT_TEXT_HEADER: u16 = 0x0F9F;
const RT_TEXT_BYTES: u16 = 0x0FA8;
const RT_USER_EDIT_ATOM: u16 = 0x0FF5;
const RT_CURRENT_USER_ATOM: u16 = 0x0FF6;
const RT_PERSIST_DIRECTORY_ATOM: u16 = 0x1772;
const SLWT_SLIDES: u16 = 0;
const SLWT_NOTES: u16 = 2;
/// The ClientTextbox container every shape's text lives under.
const RT_CLIENT_TEXTBOX: u16 = 0xF00D;

/// A `.ppt` holding one slide with `slide` as its title and `notes` as its speaker notes.
pub fn ppt(slide: &str, notes: &str) -> Vec<u8> {
    let (stream, user) = ppt_streams(slide, notes);
    cfb(&[("PowerPoint Document", &stream), ("Current User", &user)])
}

/// A `.ppt` whose `PowerPoint Document` stream declares more bytes than it holds: the slide's text
/// is still read and the loss is named.
pub fn ppt_cut(slide: &str) -> Vec<u8> {
    let (stream, user) = ppt_streams(slide, "");
    cfb_sized(&[
        ("PowerPoint Document", &stream, stream.len() as u32 + 512),
        ("Current User", &user, user.len() as u32),
    ])
}

/// A CFB container with no `PowerPoint Document` stream, named `.ppt`: the container is a
/// presentation by its name alone, so the reader gives that family's corrupt verdict.
pub fn ppt_corrupt() -> Vec<u8> {
    cfb(&[("Contents", b"not a presentation stream")])
}

/// The "PowerPoint Document" and "Current User" streams of a one-slide deck.
fn ppt_streams(slide: &str, notes: &str) -> (Vec<u8>, Vec<u8>) {
    const SLIDE_ID: u32 = 256;
    const NOTES_ID: u32 = 257;
    let mut stream = Vec::new();
    let mut doc_children = container(
        RT_SLIDE_LIST_WITH_TEXT,
        SLWT_SLIDES,
        &slide_persist(2, SLIDE_ID),
    );
    doc_children.extend(container(
        RT_SLIDE_LIST_WITH_TEXT,
        SLWT_NOTES,
        &slide_persist(3, NOTES_ID),
    ));
    let doc_offset = stream.len() as u32;
    stream.extend(container(RT_DOCUMENT, 0, &doc_children));

    // The current slide (persist id 2) and the notes page (3), reachable only through the persist
    // directory.
    let slide_offset = stream.len() as u32;
    let mut slide_children = atom(RT_SLIDE_ATOM, 0, &{
        let mut body = vec![0u8; 24];
        body[16..20].copy_from_slice(&NOTES_ID.to_le_bytes()); // notesIdRef
        body
    });
    slide_children.extend(textbox(0, slide));
    stream.extend(container(RT_SLIDE, 0, &slide_children));

    let notes_offset = stream.len() as u32;
    let mut notes_children = atom(RT_NOTES_ATOM, 0, &SLIDE_ID.to_le_bytes());
    notes_children.extend(textbox(2, notes));
    stream.extend(container(RT_NOTES, 0, &notes_children));

    let pd_offset = stream.len() as u32;
    stream.extend(persist_directory(&[
        (1, doc_offset),
        (2, slide_offset),
        (3, notes_offset),
    ]));
    let edit_offset = stream.len() as u32;
    stream.extend(user_edit_atom(pd_offset, 1));

    let user = current_user(edit_offset);
    (stream, user)
}

fn atom(rec_type: u16, instance: u16, data: &[u8]) -> Vec<u8> {
    let mut buf = (instance << 4).to_le_bytes().to_vec();
    buf.extend_from_slice(&rec_type.to_le_bytes());
    buf.extend_from_slice(&(data.len() as u32).to_le_bytes());
    buf.extend_from_slice(data);
    buf
}

fn container(rec_type: u16, instance: u16, children: &[u8]) -> Vec<u8> {
    let mut buf = ((instance << 4) | 0x000F).to_le_bytes().to_vec();
    buf.extend_from_slice(&rec_type.to_le_bytes());
    buf.extend_from_slice(&(children.len() as u32).to_le_bytes());
    buf.extend_from_slice(children);
    buf
}

/// A `ClientTextbox` holding one text run of `text_type` (`TextTypeEnum`: 0 title, 2 notes).
fn textbox(text_type: u32, text: &str) -> Vec<u8> {
    let mut children = atom(RT_TEXT_HEADER, 0, &text_type.to_le_bytes());
    children.extend(atom(RT_TEXT_BYTES, 0, text.as_bytes()));
    container(RT_CLIENT_TEXTBOX, 0, &children)
}

fn slide_persist(persist_id: u32, slide_id: u32) -> Vec<u8> {
    let mut body = persist_id.to_le_bytes().to_vec();
    body.extend_from_slice(&0u32.to_le_bytes());
    body.extend_from_slice(&0u32.to_le_bytes());
    body.extend_from_slice(&slide_id.to_le_bytes());
    body.extend_from_slice(&0u32.to_le_bytes());
    atom(RT_SLIDE_PERSIST_ATOM, 0, &body)
}

fn persist_directory(entries: &[(u32, u32)]) -> Vec<u8> {
    let header = entries[0].0 | ((entries.len() as u32) << 20);
    let mut body = header.to_le_bytes().to_vec();
    for (_, offset) in entries {
        body.extend_from_slice(&offset.to_le_bytes());
    }
    atom(RT_PERSIST_DIRECTORY_ATOM, 0, &body)
}

fn user_edit_atom(offset_persist_directory: u32, doc_persist_id_ref: u32) -> Vec<u8> {
    let mut body = 0u32.to_le_bytes().to_vec(); // lastSlideIdRef
    body.extend_from_slice(&0u32.to_le_bytes()); // version
    body.extend_from_slice(&0u32.to_le_bytes()); // offsetLastEdit
    body.extend_from_slice(&offset_persist_directory.to_le_bytes());
    body.extend_from_slice(&doc_persist_id_ref.to_le_bytes());
    body.extend_from_slice(&0u32.to_le_bytes()); // persistIdSeed
    body.extend_from_slice(&0u32.to_le_bytes()); // lastView + unused
    atom(RT_USER_EDIT_ATOM, 0, &body)
}

/// A `CurrentUserAtom` naming where the current edit starts.
fn current_user(offset_to_current_edit: u32) -> Vec<u8> {
    let mut body = 0u32.to_le_bytes().to_vec(); // size
    body.extend_from_slice(&0u32.to_le_bytes()); // headerToken: unencrypted
    body.extend_from_slice(&offset_to_current_edit.to_le_bytes());
    body.extend_from_slice(&0u16.to_le_bytes());
    body.extend_from_slice(&0u16.to_le_bytes());
    body.push(0);
    body.push(0);
    body.extend_from_slice(&0u16.to_le_bytes());
    atom(RT_CURRENT_USER_ATOM, 0, &body)
}

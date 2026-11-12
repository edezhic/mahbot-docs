//! The canonical model for PDF.
//!
//! # What is compared
//!
//! A document becomes `{ "pages": […], "info": {…}, "form": {"fields": […]}, "attachments": […] }`.
//!
//! A page is:
//!
//! - `{"media": [x0, y0, x1, y1], "crop": […], "rotate": 0}` — the boxes and the quarter turn the
//!   page resolves to. `crop` is `null` where the crop box the page resolves to repeats its media
//!   box — the format's default for a missing crop box is the media box, so the two are one page
//!   area — and a crop that differs in either direction is reported: the model compares the boxes,
//!   not the area their intersection shows
//!   (`a_crop_box_that_does_not_narrow_the_page_is_still_a_difference`);
//! - `"text": […]` — every string the page's own content stream draws, in drawing order: one entry
//!   per `Tj`/`'`/`"` and one per `TJ` (its string elements concatenated, its kerning numbers
//!   dropped), each resolved to the characters it draws through the font in force's `/ToUnicode`
//!   map. The model compares the string, not the marks it is drawn with: see "What the model does
//!   not compare";
//! - `"images": [{"sha256": …, "width": …, "height": …, "placement": [a, b, c, d, e, f]}]` — the
//!   images the content paints, by the digest of their **decoded** samples, the pixel size they
//!   decode to, and the current transform at the `Do`/`BI` that paints them: the unit square's image
//!   space maps through it, so the drawn size, the place and the turn the content gives the picture
//!   are all compared. The page's own `/Rotate` is the separate `rotate` field and is not composed
//!   into a placement. An image the model cannot read, and a `Do` of an object that is not an image,
//!   are not recorded in `images`: see "What the model does not compare";
//! - `"forms": n` — how many times the page's own content paints a form XObject: every `Do` of a
//!   form counts, and a form that paints another form is not counted again. What a form draws is
//!   not part of this field (see `appearance_text`, and "What the model does not compare");
//! - `"appearance_text": […]` — the non-empty strings the form XObjects the page paints draw, in
//!   painting order, resolved the way an appearance is (see below). A flattened field's value is
//!   here rather than in `text`, which is only what the page's own content draws;
//! - `"vector": {"count": …, "sha256": …}` — how many path operators the page draws and a digest
//!   of them with their operands;
//! - `"annots": [{"subtype": …, "rect": […], "contents": …}]` — the page's own annotations, in
//!   `/Annots` order, with `/Rect` normalised to a corner pair (or `null` when the annotation
//!   states none the model reads). An annotation that states no `/Subtype` is recorded with an
//!   empty subtype, one that states no `/Contents` with empty contents, and a form field's widget is
//!   recorded here as well as in the field list.
//!
//! The document also carries the four content-bearing Info entries (`title`, `author`, `subject`,
//! `keywords`; `null` when the document states none), every form field's fully-qualified name,
//! kind, value, options, shown appearance state and drawn appearance text, and the names of its
//! embedded files, read from the catalog's `/EmbeddedFiles` name tree.
//!
//! # How an appearance is read
//!
//! A form field's appearance draws the value the field holds, so the model reads the text its
//! widget's normal appearance stream draws, for every field kind. That stream is the widget's
//! `/AP /N` when it is a stream, or the state its `/AS` names when `/N` is a dictionary of states —
//! `Off` when it names none — so an appearance stored under a state the widget does not show is not
//! read. The characters are the ones the stream's font states in its `/ToUnicode` map — the only
//! code-to-character mapping a subset font written for PDF carries — decoded as "How a drawn string
//! is decoded" describes.
//!
//! A field's kind, flags, value and options are inherited along its `/Parent` chain, which is where
//! a widget keeps them; a dictionary whose kids name fields of their own is a container and names
//! no field itself.
//!
//! # How a drawn string is decoded
//!
//! A page's text is resolved through the font's `/ToUnicode` map where it has one — read by the
//! same CMap parser the renderer uses (`hayro-cmap`) — and falls back to the string's own bytes
//! when it has none. An appearance's text has no fallback. A code the map does not name contributes
//! the character the code stands for in a page's own run — an unpaired surrogate, which is not a
//! character, contributes nothing — and nothing in an appearance's, because an appearance must
//! state the characters it draws. A character code is read as a two-byte value whatever the map's
//! codespace states: that is what the corpus's Identity-H fonts write, an assumption about the
//! corpus rather than a property of the format, because a simple font's `/ToUnicode` map states
//! one-byte codes and would be read pairwise here. An odd trailing byte is dropped. A `/ToUnicode`
//! stream the parser cannot read, and a `Tf` naming a font the resources do not hold, are both read
//! as a font with no map. The font's identity — its name and its subset numbering — is not part of
//! the model: engines name and subset fonts differently, and only the characters are compared.
//!
//! # What is normalised away
//!
//! Each rule below is a row of the tests' `normalised_cases` table, which
//! `every_normalised_part_is_dropped` requires to compare equal; a rule the table cannot state
//! names its own test.
//!
//! - **The file's structure**: object numbers, generation numbers, `/Length` (a number or a
//!   reference to one: `a_length_stated_indirectly_is_the_same_stream`), the offsets the
//!   cross-reference index states and whether the reader follows it or rebuilds it from the
//!   objects, and stream compression — a stream is decoded before anything reads it, so
//!   Flate-compressed content and plain content are one document.
//! - **The trailer's identity**: `/ID` is never read, so two writes of one document compare equal.
//! - **The writing application**: `/CreationDate`, `/ModDate`, `/Creator`, `/Producer` and any
//!   version string an engine stamps are dropped. They name the producer, not the document.
//! - **Appearance streams** (`/AP`): a widget's appearance is generated from the field's value, so
//!   two engines may draw it differently. What is compared is the characters the appearance draws
//!   through its stated map and the appearance state a button shows, never the stream's operators
//!   (`an_appearances_geometry_and_border_are_not_compared`), and its font only through the
//!   characters it maps, as "How a drawn string is decoded" states.
//! - **The image's own encoding and the drawing operators around it**: an image is compared by its
//!   decoded samples, and its placement by the transform it is painted under, not by how the
//!   writer spelled the `q`/`cm`/`Do` sequence
//!   (`the_same_samples_are_one_image_however_they_are_stored`).
//! - **Dictionary key order**: the dictionaries the model reads for its own fields are read by key,
//!   and the comparison matches JSON objects by key, so a dictionary's key order is not a
//!   difference.
//! - **A crop box that repeats the media box** is no crop at all: see the `crop` field above
//!   (`a_crop_box_that_repeats_the_media_box_is_no_crop`).
//! - **A box's corner order**: a box written the other way round is the same box, because each
//!   axis is ordered smaller first (`a box written the other way round`).
//! - **Number spelling**: every number is held on a four-decimal grid — `(x * 10_000).round() /
//!   10_000`, ties away from zero — and `-0.0` becomes `0.0`, so `595.28`, `595.2800` and
//!   `595.28001` are one coordinate. Two numbers therefore compare equal when they round to the
//!   same grid point: a difference above `0.0001` is always a difference, one below `0.00005` is
//!   usually not one — two numbers that straddle a grid midpoint differ however close they are — and
//!   between the two it depends on where they fall. A coordinate no float can express — an overflow
//!   of composed transforms — is held as `0.0`, so it compares equal to a stated zero.
//! - **A string that states no encoding**: a PDF string is UTF-16BE when it begins with the
//!   big-endian byte-order mark `FE FF`, and is otherwise read as UTF-8 with lossy replacement, so
//!   a high byte that is not UTF-8 reads as U+FFFD. PDFDocEncoding is not decoded: two different
//!   legacy spellings can therefore read as the same replacement character and compare equal
//!   (`a_string_without_a_utf16_marker_is_read_as_utf8`).
//!
//! # What is compared byte for byte
//!
//! Nothing: an image is compared by its decoded samples, as "The image's own encoding and the
//! drawing operators around it" states.
//!
//! # What the model does not compare
//!
//! Each limit below names the test that constructs its difference, or — where the limit is a rule
//! stated under "What is normalised away" — that rule, which names its test. The README states what
//! makes a claim a limit of comparison and why a limit of reporting gets no test.
//!
//! - **A box or turn the page resolves to rather than states itself.** The model reads the values
//!   the page resolves to: a `/MediaBox`, `/CropBox` or `/Rotate` stated on the `/Pages` node is the
//!   page's own, a page with no media box anywhere is A4, and a turn that is not a multiple of 90 is
//!   no turn. A box moved from the page to its parent, a page that states no media box, and
//!   `/Rotate 45` therefore compare equal to their resolved counterparts
//!   (`a_box_inherited_from_the_page_tree_is_the_pages_own`, `a_page_with_no_media_box_is_a4`,
//!   `a_turn_that_is_not_a_quarter_is_no_turn`).
//! - **Where a page's text sits, and how it is drawn.** `text` is the strings the page draws and
//!   nothing else: the position, the font size, the colour, the character and word spacing, the
//!   render mode and the font it is drawn with are not compared — the font's identity is not part of
//!   the model, as "How a drawn string is decoded" states — so a stamp drawn in the wrong place, at
//!   the wrong size or in the wrong colour is not a difference the model sees
//!   (`page_text_is_compared_as_strings_not_as_marks`).
//! - **A page's vector content beyond its path operators.** The digest covers the path operators
//!   and their operands; the colour, the line width, the text state and the transform the path is
//!   drawn under are not in it, so the same path drawn in another colour or at another place
//!   compares equal (`a_vector_digest_ignores_colour_width_and_transform`).
//! - **A form XObject's own content.** A page records how many times it paints a form, and the
//!   text those forms draw; a form's images and path operators are not recorded at all, so a form
//!   that draws only vector marks is indistinguishable from a form that draws nothing, and two
//!   different drawings are one page
//!   (`a_form_that_draws_only_marks_draws_nothing_the_model_reads`,
//!   `a_painted_form_is_counted_and_its_drawing_is_not_compared`). A form's or an appearance's
//!   text is read through its **own** `/Resources`: the format lets a form inherit the page's and
//!   an appearance the AcroForm's `/DR` when it states none, and the model does neither, so a
//!   stream without resources states no text (`a_form_without_its_own_resources_states_no_text`,
//!   `an_appearance_does_not_inherit_the_form_default_resources`).
//! - **An appearance's geometry and border.** An appearance is compared by the characters it
//!   draws, not by where it draws them: the line positions, the font size, the appearance's own
//!   `/BBox` — the box it draws in, not the widget's `/Rect`, which `annotations` records — and the
//!   widget's `/BS` border and padding are not part of the model, whatever the field kind
//!   (`an_appearances_geometry_and_border_are_not_compared`).
//! - **An appearance's font when the file does not embed it.** The stream's font must carry an
//!   embedded program (`/FontFile`, `/FontFile2` or `/FontFile3`) for the appearance to be read at
//!   all, so a widget whose font carries none states no text and two such widgets compare equal
//!   whatever their maps say (`an_appearance_drawn_with_an_unembedded_font_states_nothing`).
//! - **An appearance's glyph program.** The characters come from the stated map; the outlines the
//!   program carries are not compared, so two files whose programs differ — a subset missing a
//!   glyph included — state the same value and compare equal. Nor does the model check that the
//!   program carries a glyph for every code the appearance draws: a code the stated map names
//!   decodes to its character whatever the program holds
//!   (`an_appearances_glyph_program_is_not_compared`).
//! - **A field's `/DA`.** It states what a viewer would edit the field with, not what the file
//!   shows (`a_fields_default_appearance_is_not_read`).
//! - **A field's value, kind, state and options, read the way the format's writers read them.** A
//!   field with no `/FT` and one whose type the model does not name are both `unknown`; a choice
//!   that selects several options holds them as an array and the model spells them joined with
//!   `", "`, so an array and a string that spells the join are one value; a choice's — or a push
//!   button's — `[export display]` pair is its display name, the second entry or the first when the
//!   pair states one, so the export name is not compared; a radio group's or a checkbox's `/Opt`
//!   holds export values, so a pair there names none; a radio group's state is the first state its
//!   widgets show that is not `Off` — `Off` when it has widgets and none shows another state,
//!   including when they state no `/AS` at all, and `null` when the group has no widget; a
//!   checkbox's state is its first widget's, `null` when that widget states none
//!   (`a_field_without_a_type_is_unknown`,
//!   `a_multi_select_value_is_spelled_as_one_string`,
//!   `an_option_pairs_export_name_is_not_compared`,
//!   `a_radio_state_is_the_first_state_that_is_not_off`).
//! - **An annotation beyond its subtype, rectangle and contents.** The colour, the title, the
//!   flags and a link's action target are not read, so a difference confined to them compares
//!   equal (`an_annotation_is_its_subtype_rectangle_and_contents`).
//! - **The parts of the document the model does not read at all**: outlines, page labels, the
//!   catalog's language and XMP metadata. A difference confined to them is not seen
//!   (`outlines_page_labels_language_and_xmp_are_not_read`).
//! - **The document's structure tree and a page's marked content** are not read either: a document
//!   that tags its content and one that does not are one document
//!   (`a_structure_tree_and_marked_content_are_not_read`).
//! - **An attachment beyond its name.** `attachments` holds the names in the catalog's
//!   `/EmbeddedFiles` name tree and nothing else: a file specification's bytes are not read, so two
//!   attachments of one name whose files differ compare equal
//!   (`an_attachment_is_its_name_and_not_its_file`).
//! - **A number below the model's grid**: see "Number spelling" above for the boundary, and for the
//!   coordinate no float can express (`numbers_are_compared_on_a_four_decimal_grid`).
//! - **A string in a legacy encoding**: see "A string that states no encoding" above.
//! - **An image the model cannot decode, one that states no positive width or height, and a `Do` of
//!   an object that is neither an image nor a form.** None of them is recorded, so a page that
//!   paints only one of them compares equal to a page that paints nothing
//!   (`an_image_the_model_cannot_decode_is_not_recorded`).
//! - **A content stream the model cannot decode.** A page's or a form's stream that the decoder
//!   refuses contributes nothing the model records, so it models as one that draws nothing
//!   (`a_content_stream_the_model_cannot_decode_draws_nothing`).
//! - **What lies beyond a bound, and what follows an unreadable entry.** The annotation, field,
//!   option and attachment lists, the field tree, a name tree and a form nesting all stop at a
//!   bound, so a difference past it compares equal (`every_list_bound_cuts_off_what_lies_beyond_it`);
//!   an entry of `/Annots` or `/AcroForm/Fields` that the file cannot resolve ends the list rather
//!   than being skipped, so everything after it is not modelled
//!   (`an_unresolvable_reference_ends_the_list_it_is_in`).
//!
//! One limit of reporting is not a limit of comparison, so it has no test that requires a difference
//! to compare equal: the model cannot read the words of a page that draws them as outlines — such a
//! page states no text, so the model compares the outlines as path operators
//! (`text_drawn_as_outlines_is_read_as_vector_content` pins what it does read).
//!
//! # What the model refuses
//!
//! A document of more than 10,000 pages, or a content stream that runs more than 100,000
//! operators, is an error rather than a model. The page bound limits what the model will describe,
//! not what the parser allocates: the parser materialises the page list before the bound is applied
//! and recurses through the page tree, so a deeply nested tree overflows its stack rather than
//! reaching the bound — an abort that no containment can catch. The operator bound is charged per
//! stream, so a stream that never ends is refused rather than exhausting the run.
//!
//! Every stream the model decodes is charged its decoded length against one document-wide total
//! (256 MiB): a page's content, a form XObject, a font's `/ToUnicode` map. An image is charged at
//! four bytes per declared pixel **before** it is decoded (64 MiB for one image), so a stream that
//! claims an enormous size is refused rather than allocated. A stream object painted as an image or
//! a form is charged once however often it is painted — it is still decoded on each paint, which is
//! work rather than memory — and an inline image, which has no object identity, is charged on every
//! paint. A page's content stream is charged per page that reads it. A non-image stream is decoded
//! before it can be charged, so one stream's own expansion is the decoder's; the charge refuses the
//! document rather than letting a document of many streams exhaust the run.
//!
//! An unreadable PDF is an `Err`, never an empty value: a comparison must not pass because the
//! model could not read what it was given.

use crate::util::hex_string;
use anyhow::{Result, anyhow, bail};
use hayro::hayro_syntax::Pdf;
use hayro::hayro_syntax::content::{Instruction, UntypedIter};
use hayro::hayro_syntax::metadata::Metadata;
use hayro::hayro_syntax::object::dict::Dict;
use hayro::hayro_syntax::object::stream::{ImageDecodeParams, Stream};
use hayro::hayro_syntax::object::{Array, Name, Object, ObjectIdentifier, String as PdfString};
use hayro::hayro_syntax::page::{Page, Rotation};
use hayro_cmap::{BfString, CMap};
use serde_json::{Value, json};
use sha2::{Digest as _, Sha256};
use std::collections::{HashMap, HashSet};
use std::rc::Rc;

/// The decimals every number in the model keeps — four, the precision a PDF's own arithmetic
/// holds rather than the last bit of a float's spelling — as the scale that rounds to them. The
/// module docs state what that makes equal.
const ROUND_FACTOR: f64 = 10_000.0;

/// Maximum annotations modelled for one page; a longer `/Annots` array is cut off here rather than
/// walked, so a cycle terminates.
const MAX_ANNOTATIONS: usize = 1000;

/// Maximum form fields modelled for one document; a longer field tree is cut off here.
const MAX_FIELDS: usize = 1000;

/// The depth at which the field-tree walk stops: a field 32 levels below its root is read, one 33
/// levels down is not, so `/Kids` that point in a cycle terminate.
const MAX_FIELD_DEPTH: usize = 32;

/// The depth at which the appearance walk stops: a form nine deep is followed, one ten deep is not,
/// so a form that paints itself terminates.
const MAX_FORM_DEPTH: usize = 8;

/// Maximum options listed for one field; a longer `/Opt` is cut off here.
const MAX_FIELD_OPTIONS: usize = 100;

/// The depth at which a name tree is walked: a name eight levels below the root is read, one nine
/// levels down is not, so `/Kids` that point in a cycle terminate.
const MAX_NAME_TREE_DEPTH: usize = 8;

/// Maximum embedded files named for one document; a longer name tree is cut off here.
const MAX_ATTACHMENTS: usize = 1000;

/// Maximum pages one document may hold, so a document with more is refused rather than modelled.
/// This is the model's own bound, not the kit's: `rules.json`'s `max_pages_per_request` bounds the
/// pages a request may name, which is a different quantity.
const MAX_PAGES: usize = 10_000;

/// Maximum content-stream operators one stream may run. Each stream the model walks — a page's own
/// content and each form it paints — is charged its own count.
const MAX_STREAM_OPERATORS: usize = 100_000;

/// Maximum bytes one image may claim to decode to.
const MAX_IMAGE_BYTES: u64 = 64 * 1024 * 1024;

/// Maximum bytes one document's decoded streams may hold in total. Independent of [`MAX_PAGES`] — a
/// document may be refused by either bound, and the refusal names the one it crossed.
const MAX_DECODED_BYTES: u64 = 256 * 1024 * 1024;

/// `/Ff` bit 16: a `Btn` field is a radio group, whose kids are its buttons.
const FIELD_FLAG_RADIO: i32 = 1 << 15;

/// `/Ff` bit 17: a `Btn` field is a push button, which holds no value a fill could set.
const FIELD_FLAG_PUSH_BUTTON: i32 = 1 << 16;

/// The content-stream operators that build, clip and paint a path — what the model digests as a
/// page's vector content.
const VECTOR_OPERATORS: &[&[u8]] = &[
    // Construction.
    b"m", b"l", b"c", b"v", b"y", b"h", b"re", //
    // Clipping.
    b"W", b"W*", //
    // Painting.
    b"S", b"s", b"f", b"F", b"f*", b"B", b"B*", b"b", b"b*", b"n",
];
/// The parsed `/ToUnicode` maps of one page, by font object, so a page with many text runs parses
/// each CMap once.
type ToUnicodeCache = HashMap<ObjectIdentifier, Option<Rc<CMap>>>;

/// The canonical form of one PDF's bytes.
pub fn canonical(bytes: &[u8]) -> Result<Value> {
    // The parser is third-party code running over untrusted bytes; a panic inside it would unwind
    // through the caller, and a caught one must not read as a model of anything. The shared
    // containment keeps the panic's own message and its trace off the harness's stderr. A stack
    // overflow inside the parser aborts the process, which no containment can catch.
    match crate::shutdown::contain_panics(|| model(bytes)) {
        Ok(value) => value,
        Err(payload) => Err(anyhow!(
            "the PDF parser panicked ({}), so the document is undecidable",
            crate::shutdown::panic_message(&*payload)
        )),
    }
}

/// The model itself, over bytes that may still make the parser panic.
fn model(bytes: &[u8]) -> Result<Value> {
    let pdf = Pdf::new(bytes.to_vec())
        .map_err(|error| anyhow!("the bytes are not a readable PDF ({error:?})"))?;
    let pages = pdf.pages();
    charge_pages(pages.len())?;
    let mut budget = Decoded::default();
    let pages = pages
        .iter()
        .map(|page| page_value(page, &mut budget))
        .collect::<Result<Vec<_>>>()?;
    let fields = fields_value(&pdf, &mut budget)?;
    Ok(json!({
        "pages": pages,
        "info": info_value(pdf.metadata()),
        "form": { "fields": fields },
        "attachments": attachment_names(&pdf),
    }))
}

/// Refuse a document over the page bound.
fn charge_pages(count: usize) -> Result<()> {
    if count > MAX_PAGES {
        bail!("the document holds more than {MAX_PAGES} pages");
    }
    Ok(())
}

/// Count one content-stream operator against the bound of the stream it belongs to.
fn charge_operators(count: &mut usize) -> Result<()> {
    *count += 1;
    if *count > MAX_STREAM_OPERATORS {
        bail!("a content stream runs more than {MAX_STREAM_OPERATORS} operators");
    }
    Ok(())
}

/// The decoded bytes one document has been charged, and the stream objects already charged, so a
/// stream painted many times is one charge.
#[derive(Default)]
struct Decoded {
    total: u64,
    charged: HashSet<ObjectIdentifier>,
}

impl Decoded {
    /// Charge `bytes` against the document's total.
    fn charge(&mut self, bytes: u64) -> Result<()> {
        self.total = self.total.saturating_add(bytes);
        if self.total > MAX_DECODED_BYTES {
            bail!("the document's decoded streams exceed the {MAX_DECODED_BYTES}-byte total");
        }
        Ok(())
    }

    /// Charge a decoded stream, once per stream object.
    fn stream(&mut self, stream: &Stream<'_>, bytes: u64) -> Result<()> {
        if self.first(stream.dict().obj_id()) {
            self.charge(bytes)?;
        }
        Ok(())
    }

    /// Charge an image's declared pixel area, at four bytes per pixel, **before** the decode is
    /// attempted: the charge is what the decode may allocate.
    fn image(&mut self, stream: &Stream<'_>, width: u32, height: u32) -> Result<()> {
        if self.first(stream.dict().obj_id()) {
            self.image_area(width, height)?;
        }
        Ok(())
    }

    /// Charge an image's declared area unconditionally, saturating rather than wrapping: a crafted
    /// `/Width` of `u32::MAX` must be refused, not charged as zero.
    fn image_area(&mut self, width: u32, height: u32) -> Result<()> {
        let claimed = u64::from(width)
            .saturating_mul(u64::from(height))
            .saturating_mul(4);
        if claimed > MAX_IMAGE_BYTES {
            bail!("an image claims more than the {MAX_IMAGE_BYTES}-byte decode limit");
        }
        self.charge(claimed)
    }

    /// Whether this stream object has not been charged yet. An object without an identifier — an
    /// inline image — is always charged: the model cannot tell two of them apart.
    fn first(&mut self, id: Option<ObjectIdentifier>) -> bool {
        match id {
            Some(id) => self.charged.insert(id),
            None => true,
        }
    }
}

/// Charge the page's own content stream(s) against the document's total. `page_stream()` decodes
/// `/Contents` once and caches it — a single stream or an array joined as the format defines — so
/// the operator walk below reads the same bytes instead of decoding them again. A stream that
/// cannot be decoded is never walked, so nothing is charged for it.
fn charge_content(page: &Page<'_>, budget: &mut Decoded) -> Result<()> {
    budget.charge(page.page_stream().map_or(0, |stream| stream.len() as u64))
}

/// One page: the area it shows, the quarter turn it is displayed at, what it draws and what is
/// marked on it.
fn page_value(page: &Page<'_>, budget: &mut Decoded) -> Result<Value> {
    let media = page.media_box();
    let crop = page.crop_box();
    let media_value = box_value(media.x0, media.y0, media.x1, media.y1);
    let crop_value = box_value(crop.x0, crop.y0, crop.x1, crop.y1);
    // A page's visible area is its crop box, and the format's own default for a crop box is the
    // media box. A crop that repeats the media box therefore says nothing the media box does not,
    // and neither does the absence of one — the model reports the crop only where it differs, so a
    // writer that drops a redundant crop (which is what the kit does when it copies a page) is not
    // a difference.
    let crop = if crop_value == media_value {
        Value::Null
    } else {
        crop_value
    };

    let mut text: Vec<Value> = Vec::new();
    let mut images: Vec<Value> = Vec::new();
    let mut appearance_text: Vec<Value> = Vec::new();
    let mut forms = 0_usize;
    let mut vector = Sha256::new();
    let mut vector_count = 0_usize;
    // The font the following text operators draw with, and the maps already parsed for this page.
    let mut font: Option<Dict<'_>> = None;
    let mut unicode_maps: ToUnicodeCache = HashMap::new();
    // The transform the content paints under: `cm` composes onto it, and `q`/`Q` save and restore
    // it. An image's placement is this transform at the `Do` that paints it.
    let mut ctm = Matrix::IDENTITY;
    let mut saved: Vec<Matrix> = Vec::new();
    charge_content(page, budget)?;
    let mut operations = page.operations();
    let mut operators = 0_usize;
    while let Some(instruction) = operations.next() {
        charge_operators(&mut operators)?;
        let operator: &[u8] = instruction.operator;
        match operator {
            // The graphics state the page paints under. `q`/`Q` do not nest further than the stack
            // a stream states, which the operator bound already limits.
            b"q" => saved.push(ctm),
            b"Q" => {
                if let Some(previous) = saved.pop() {
                    ctm = previous;
                }
            }
            b"cm" => {
                if let Some(transform) = matrix_of(&instruction) {
                    ctm = transform.compose(&ctm);
                }
            }
            // `Tf` names the font the following strings are drawn with. `get_font` walks the
            // resource's parent chain, so an inherited font is found as well.
            b"Tf" => {
                font = instruction
                    .operands()
                    .next()
                    .and_then(|operand| match operand {
                        Object::Name(name) => page.resources().get_font(name),
                        _ => None,
                    });
            }
            // Every text-showing operator draws one run; `TJ`'s string elements are concatenated
            // and its kerning numbers dropped.
            b"Tj" | b"'" | b"\"" | b"TJ" => {
                if let Some(drawn) = instruction_text(
                    &instruction,
                    font.as_ref(),
                    &mut unicode_maps,
                    budget,
                    RunText::Page,
                )? {
                    text.push(Value::from(drawn));
                }
            }
            b"Do" => {
                if let Some(Object::Name(name)) = instruction.operands().next()
                    && let Some(stream) = page.resources().get_x_object(name)
                {
                    if stream.dict().get::<Name<'_>>(b"Subtype").as_deref() == Some(b"Form") {
                        forms += 1;
                        walk_form(&stream, &mut appearance_text, &mut unicode_maps, budget, 0)?;
                    } else if let Some(image) = image_value(&stream, budget, &ctm)? {
                        images.push(image);
                    }
                }
            }
            // An inline image arrives as the one stream operand of its `BI`.
            b"BI" => {
                if let Some(Object::Stream(stream)) = instruction.operands().next()
                    && let Some(image) = image_value(stream, budget, &ctm)?
                {
                    images.push(image);
                }
            }
            _ => {
                if VECTOR_OPERATORS.contains(&operator) {
                    vector_count += 1;
                    let mut line = String::from_utf8_lossy(operator).into_owned();
                    for operand in instruction.operands() {
                        line.push(' ');
                        operand_text(operand, &mut line);
                    }
                    vector.update(line.as_bytes());
                    vector.update(b"\n");
                }
            }
        }
    }

    Ok(json!({
        "media": media_value,
        "crop": crop,
        "rotate": rotation_degrees(page.rotation()),
        "text": text,
        "images": images,
        "forms": forms,
        "appearance_text": appearance_text,
        "vector": { "count": vector_count, "sha256": hex_string(&vector.finalize()) },
        "annots": annotations_value(page),
    }))
}

/// A PDF transform as the format states it: `[a b c d e f]`, the matrix that maps a point `(x, y)`
/// to `(a x + c y + e, b x + d y + f)`.
#[derive(Clone, Copy, Debug)]
struct Matrix {
    a: f64,
    b: f64,
    c: f64,
    d: f64,
    e: f64,
    f: f64,
}

impl Matrix {
    /// The identity: the unit square stays where it is.
    const IDENTITY: Self = Self {
        a: 1.0,
        b: 0.0,
        c: 0.0,
        d: 1.0,
        e: 0.0,
        f: 0.0,
    };

    /// This transform composed onto `outer`: a point is mapped by `self` and then by `outer`, which
    /// is what the format's `cm` states.
    fn compose(&self, outer: &Self) -> Self {
        Self {
            a: self.a * outer.a + self.b * outer.c,
            b: self.a * outer.b + self.b * outer.d,
            c: self.c * outer.a + self.d * outer.c,
            d: self.c * outer.b + self.d * outer.d,
            e: self.e * outer.a + self.f * outer.c + outer.e,
            f: self.e * outer.b + self.f * outer.d + outer.f,
        }
    }

    /// The transform as the model holds it: six coordinates, each rounded like every other number.
    fn value(self) -> Value {
        json!([
            rounded(self.a),
            rounded(self.b),
            rounded(self.c),
            rounded(self.d),
            rounded(self.e),
            rounded(self.f),
        ])
    }
}

/// The six numbers of a `cm`, or `None` when the operator does not state six numbers.
fn matrix_of(instruction: &hayro::hayro_syntax::content::Instruction<'_, '_>) -> Option<Matrix> {
    let numbers = instruction
        .operands()
        .map(|operand| match operand {
            Object::Number(number) => Some(number.as_f64()),
            _ => None,
        })
        .collect::<Option<Vec<f64>>>()?;
    match numbers.as_slice() {
        [a, b, c, d, e, f] => Some(Matrix {
            a: *a,
            b: *b,
            c: *c,
            d: *d,
            e: *e,
            f: *f,
        }),
        _ => None,
    }
}

/// The quarter turn a page is displayed at, in degrees clockwise.
fn rotation_degrees(rotation: Rotation) -> i32 {
    match rotation {
        Rotation::None => 0,
        Rotation::Horizontal => 90,
        Rotation::Flipped => 180,
        Rotation::FlippedHorizontal => 270,
    }
}

/// A box as `[x0, y0, x1, y1]`, each axis from its smaller coordinate to its larger.
fn box_value(x0: f64, y0: f64, x1: f64, y1: f64) -> Value {
    json!([
        rounded(x0.min(x1)),
        rounded(y0.min(y1)),
        rounded(x0.max(x1)),
        rounded(y0.max(y1)),
    ])
}

/// One number as the model holds it: on the four-decimal grid [`ROUND_FACTOR`] names, with `-0.0`
/// folded onto `0.0`.
fn rounded(value: f64) -> f64 {
    // A coordinate no float can express — what a composition of large transforms overflows to —
    // is held as zero. serde_json writes such a value as `null`, which two pages would then share
    // for the wrong reason; zero is at least a coordinate, and the module docs state that it
    // compares equal to a stated zero.
    if !value.is_finite() {
        return 0.0;
    }
    let rounded = (value * ROUND_FACTOR).round() / ROUND_FACTOR;
    // The two zeros are one coordinate, and serde_json would spell them differently.
    if rounded == 0.0 { 0.0 } else { rounded }
}

/// A PDF text string: UTF-16BE when it carries the byte-order mark, otherwise the bytes read as
/// UTF-8 with lossy replacement. PDFDocEncoding is not decoded, so a high byte outside UTF-8 reads
/// as U+FFFD; the module docs state what that costs the comparison.
fn pdf_text(bytes: &[u8]) -> String {
    if let Some(rest) = bytes.strip_prefix(&[0xFE, 0xFF]) {
        let units = rest
            .as_chunks::<2>()
            .0
            .iter()
            .map(|pair| u16::from_be_bytes(*pair))
            .collect::<Vec<_>>();
        return String::from_utf16_lossy(&units);
    }
    String::from_utf8_lossy(bytes).into_owned()
}

/// How a text run is read: a page's own content, or an appearance. The module docs state what each
/// does with a code its font's map does not name.
#[derive(Clone, Copy, PartialEq, Eq)]
enum RunText {
    Page,
    Appearance,
}

/// The characters a drawn string stands for: its font's `/ToUnicode` map when the font has one,
/// otherwise, for a page's own content, the string's own bytes.
fn run_text<'a>(
    bytes: &[u8],
    font: Option<&Dict<'a>>,
    cache: &mut ToUnicodeCache,
    budget: &mut Decoded,
    mode: RunText,
) -> Result<String> {
    let font = match mode {
        RunText::Page => font,
        RunText::Appearance => font.filter(|font| font_is_embedded(font)),
    };
    let Some(map) = font
        .map(|font| to_unicode_map(font, cache, budget))
        .transpose()?
        .flatten()
    else {
        return Ok(if mode == RunText::Page {
            pdf_text(bytes)
        } else {
            String::new()
        });
    };
    let mut out = String::new();
    for pair in bytes.as_chunks::<2>().0 {
        let code = u16::from_be_bytes(*pair);
        match map.lookup_bf_string(u32::from(code)) {
            Some(BfString::Char(character)) => out.push(character),
            Some(BfString::String(text)) => out.push_str(&text),
            None if mode == RunText::Page => {
                if let Some(character) = char::from_u32(u32::from(code)) {
                    out.push(character);
                }
            }
            None => {}
        }
    }
    Ok(out)
}

/// The run a text-showing instruction draws, or `None` when it is not one. `"` states its string
/// last; the module docs state what `TJ`'s numbers and strings mean.
fn instruction_text<'b, 'a>(
    instruction: &Instruction<'b, 'a>,
    font: Option<&Dict<'a>>,
    cache: &mut ToUnicodeCache,
    budget: &mut Decoded,
    mode: RunText,
) -> Result<Option<String>> {
    let operator: &[u8] = instruction.operator;
    match operator {
        b"Tj" | b"'" => match instruction.operands().next() {
            Some(Object::String(drawn)) => {
                Ok(Some(run_text(drawn.as_bytes(), font, cache, budget, mode)?))
            }
            _ => Ok(None),
        },
        b"\"" => match instruction.operands().last() {
            Some(Object::String(drawn)) => {
                Ok(Some(run_text(drawn.as_bytes(), font, cache, budget, mode)?))
            }
            _ => Ok(None),
        },
        b"TJ" => {
            let Some(Object::Array(parts)) = instruction.operands().next() else {
                return Ok(None);
            };
            let mut line = String::new();
            for part in parts.iter::<Object<'_>>() {
                if let Object::String(drawn) = part {
                    line.push_str(&run_text(drawn.as_bytes(), font, cache, budget, mode)?);
                }
            }
            Ok(Some(line))
        }
        _ => Ok(None),
    }
}

/// The font's parsed `/ToUnicode` map, if it has a decodable one. The parse happens once per font
/// object: the page's later runs read the cached map.
fn to_unicode_map<'a>(
    font: &Dict<'a>,
    cache: &mut ToUnicodeCache,
    budget: &mut Decoded,
) -> Result<Option<Rc<CMap>>> {
    let key = font.obj_id();
    if let Some(key) = key
        && let Some(cached) = cache.get(&key)
    {
        return Ok(cached.clone());
    }
    let Some(stream) = font.get::<Stream<'a>>(b"ToUnicode") else {
        return Ok(None);
    };
    let Ok(data) = stream.decoded() else {
        return Ok(None);
    };
    budget.stream(&stream, data.len() as u64)?;
    let parsed = CMap::parse(data.as_ref(), |_| None).map(Rc::new);
    if let Some(key) = key {
        cache.insert(key, parsed.clone());
    }
    Ok(parsed)
}

/// One operand of a path operator, as the digest reads it. Every variant is total, because a
/// content stream may put anything under a path operator.
fn operand_text(object: &Object<'_>, out: &mut String) {
    match object {
        Object::Null(_) => out.push_str("null"),
        Object::Boolean(value) => out.push_str(if *value { "true" } else { "false" }),
        Object::Number(number) => {
            let value = rounded(number.as_f64());
            out.push_str(&format!("{value}"));
        }
        Object::String(text) => {
            out.push('(');
            out.push_str(&pdf_text(text.as_bytes()));
            out.push(')');
        }
        Object::Name(name) => {
            out.push('/');
            out.push_str(&String::from_utf8_lossy(name));
        }
        Object::Array(items) => {
            out.push('[');
            for (index, item) in items.iter::<Object<'_>>().enumerate() {
                if index > 0 {
                    out.push(' ');
                }
                operand_text(&item, out);
            }
            out.push(']');
        }
        Object::Dict(dict) => {
            out.push('<');
            for key in dict.keys() {
                out.push_str(&String::from_utf8_lossy(&key));
                out.push(' ');
                if let Some(value) = dict.get::<Object<'_>>(&key) {
                    operand_text(&value, out);
                }
                out.push(';');
            }
            out.push('>');
        }
        // A stream is never a path operand; naming it keeps the digest total.
        Object::Stream(_) => out.push_str("stream"),
    }
}

/// The strings one painted form XObject draws, appended in drawing order.
///
/// A form that paints another form is followed, bounded by [`MAX_FORM_DEPTH`]; the module docs
/// state what the walk does not read.
fn walk_form(
    stream: &Stream<'_>,
    out: &mut Vec<Value>,
    cache: &mut ToUnicodeCache,
    budget: &mut Decoded,
    depth: usize,
) -> Result<()> {
    if depth > MAX_FORM_DEPTH {
        return Ok(());
    }
    let Ok(bytes) = stream.decoded() else {
        return Ok(());
    };
    budget.stream(stream, bytes.len() as u64)?;
    let fonts = stream
        .dict()
        .get::<Dict<'_>>(b"Resources")
        .and_then(|resources| resources.get::<Dict<'_>>(b"Font"));
    let mut font: Option<Dict<'_>> = None;
    let mut operators = 0_usize;
    let mut operations = UntypedIter::new(bytes.as_ref());
    while let Some(instruction) = operations.next() {
        charge_operators(&mut operators)?;
        let operator: &[u8] = instruction.operator;
        match operator {
            b"Tf" => {
                font = instruction
                    .operands()
                    .next()
                    .and_then(|operand| match operand {
                        Object::Name(name) => fonts.as_ref().and_then(|fonts| fonts.get(name)),
                        _ => None,
                    });
            }
            b"Tj" | b"'" | b"\"" | b"TJ" => {
                if let Some(drawn) = instruction_text(
                    &instruction,
                    font.as_ref(),
                    cache,
                    budget,
                    RunText::Appearance,
                )? && !drawn.is_empty()
                {
                    out.push(Value::from(drawn));
                }
            }
            b"Do" => {
                if let Some(Object::Name(name)) = instruction.operands().next()
                    && let Some(nested) = stream
                        .dict()
                        .get::<Dict<'_>>(b"Resources")
                        .and_then(|resources| resources.get::<Dict<'_>>(b"XObject"))
                        .and_then(|objects| objects.get::<Stream<'_>>(name))
                    && nested.dict().get::<Name<'_>>(b"Subtype").as_deref() == Some(b"Form")
                {
                    walk_form(&nested, out, cache, budget, depth + 1)?;
                }
            }
            _ => {}
        }
    }
    Ok(())
}

/// Whether a font carries its own outline program: a `/FontFile`, `/FontFile2` or `/FontFile3` on
/// its descriptor, or on the descendant of a composite font.
fn font_is_embedded(font: &Dict<'_>) -> bool {
    fn descriptor_holds_program(descriptor: &Dict<'_>) -> bool {
        ["FontFile", "FontFile2", "FontFile3"]
            .iter()
            .any(|key| descriptor.contains_key(key.as_bytes()))
    }
    if font
        .get::<Dict<'_>>(b"FontDescriptor")
        .is_some_and(|descriptor| descriptor_holds_program(&descriptor))
    {
        return true;
    }
    font.get::<Array<'_>>(b"DescendantFonts")
        .and_then(|descendants| {
            descendants
                .iter::<Object<'_>>()
                .find_map(|descendant| match descendant {
                    Object::Dict(descendant) => Some(descendant),
                    _ => None,
                })
        })
        .and_then(|descendant| descendant.get::<Dict<'_>>(b"FontDescriptor"))
        .is_some_and(|descriptor| descriptor_holds_program(&descriptor))
}

/// One image stream — an image XObject or an inline image — as the model holds it: the digest of
/// its decoded samples, the pixel size they decode to, and the transform it is painted under.
/// `None` for a stream whose stated subtype is not `/Image`, one that states no positive width or
/// height, or one that cannot be decoded.
fn image_value(
    stream: &Stream<'_>,
    budget: &mut Decoded,
    placement: &Matrix,
) -> Result<Option<Value>> {
    let dict = stream.dict();
    // A form XObject draws content of its own and carries no samples. An inline image states no
    // subtype at all, so only a stated non-image subtype is skipped.
    if dict
        .get::<Name<'_>>(b"Subtype")
        .as_deref()
        .is_some_and(|subtype| subtype != b"Image")
    {
        return Ok(None);
    }
    // An inline image abbreviates both keys.
    let Some(width) = dict
        .get::<u32>(b"Width")
        .or_else(|| dict.get::<u32>(b"W"))
        .filter(|width| *width > 0)
    else {
        return Ok(None);
    };
    let Some(height) = dict
        .get::<u32>(b"Height")
        .or_else(|| dict.get::<u32>(b"H"))
        .filter(|height| *height > 0)
    else {
        return Ok(None);
    };
    // Charged before the decode: what an image claims to decode to is what the decode may allocate.
    budget.image(stream, width, height)?;
    let params = ImageDecodeParams {
        width,
        height,
        ..ImageDecodeParams::default()
    };
    let Ok(decoded) = stream.decoded_image(&params) else {
        return Ok(None);
    };
    // What a decoder reports about its own output is authoritative over the dictionary.
    let (width, height) = match &decoded.image_data {
        Some(data) => (data.width, data.height),
        None => (width, height),
    };
    Ok(Some(json!({
        "sha256": hex_string(&Sha256::digest(decoded.data.as_ref())),
        "width": width,
        "height": height,
        "placement": placement.value(),
    })))
}

/// The page's own annotations, in `/Annots` order.
fn annotations_value(page: &Page<'_>) -> Vec<Value> {
    let Some(annots) = page.raw().get::<Array<'_>>(b"Annots") else {
        return Vec::new();
    };
    let mut out = Vec::new();
    let mut annots = annots.flex_iter();
    while let Some(object) = annots.next::<Object<'_>>() {
        if out.len() == MAX_ANNOTATIONS {
            break;
        }
        let Some(dict) = object.into_dict() else {
            continue;
        };
        let subtype = dict
            .get::<Name<'_>>(b"Subtype")
            .map(|subtype| String::from_utf8_lossy(&subtype).into_owned())
            .unwrap_or_default();
        let contents = dict
            .get::<PdfString>(b"Contents")
            .map(|text| pdf_text(text.as_bytes()))
            .unwrap_or_default();
        out.push(json!({
            "subtype": subtype,
            "rect": rect_value(&dict),
            "contents": contents,
        }));
    }
    out
}

/// An annotation's `/Rect` as a corner pair, or `null` when it states none the model can read.
fn rect_value(dict: &Dict<'_>) -> Value {
    let Some(corners) = dict.get::<Array<'_>>(b"Rect") else {
        return Value::Null;
    };
    let numbers = corners
        .iter::<Object<'_>>()
        .filter_map(|object| match object {
            Object::Number(number) => Some(number.as_f64()),
            _ => None,
        })
        .collect::<Vec<_>>();
    match numbers.as_slice() {
        [x0, y0, x1, y1] => box_value(*x0, *y0, *x1, *y1),
        _ => Value::Null,
    }
}

/// The document's own description: the four Info entries that say what the document is about.
/// Dates, the creator and the producer name the writing application and are dropped.
fn info_value(metadata: &Metadata) -> Value {
    fn entry(value: &Option<Vec<u8>>) -> Value {
        match value {
            Some(bytes) => Value::from(pdf_text(bytes)),
            None => Value::Null,
        }
    }
    json!({
        "title": entry(&metadata.title),
        "author": entry(&metadata.author),
        "subject": entry(&metadata.subject),
        "keywords": entry(&metadata.keywords),
    })
}

/// Every form field, in field-tree order.
fn fields_value(pdf: &Pdf, budget: &mut Decoded) -> Result<Vec<Value>> {
    let Some(acroform) = catalog(pdf).and_then(|catalog| catalog.get::<Dict<'_>>(b"AcroForm"))
    else {
        return Ok(Vec::new());
    };
    let Some(fields) = acroform.get::<Array<'_>>(b"Fields") else {
        return Ok(Vec::new());
    };
    let mut walk = FieldWalk {
        found: Vec::new(),
        visited: HashSet::new(),
    };
    for field in fields.iter::<Object<'_>>() {
        if let Object::Dict(field) = field {
            walk.visit(&field, "", 0);
        }
    }
    let mut cache: ToUnicodeCache = HashMap::new();
    let mut out = Vec::new();
    for (field, name) in walk.found {
        let kind = field_kind(&field);
        out.push(json!({
            "name": name,
            "kind": kind,
            "value": field_value(&field),
            "options": field_options(&field, kind),
            "state": field_state(&field, kind),
            "appearance": field_appearance(&field, &mut cache, budget)?,
        }));
    }
    Ok(out)
}

/// The document catalog, which holds the form and the name tree.
fn catalog<'a>(pdf: &'a Pdf) -> Option<Dict<'a>> {
    pdf.xref().get::<Dict<'a>>(pdf.xref().root_id())
}

/// One pass over the field tree: the fields it reaches and the objects it has already entered, so
/// `/Kids` that point back terminate.
struct FieldWalk<'a> {
    found: Vec<(Dict<'a>, String)>,
    visited: HashSet<(usize, usize)>,
}

impl<'a> FieldWalk<'a> {
    /// Visit `field` and, through it, the named kids below it. `prefix` is the qualified name its
    /// ancestors established.
    fn visit(&mut self, field: &Dict<'a>, prefix: &str, depth: usize) {
        if depth > MAX_FIELD_DEPTH || self.found.len() >= MAX_FIELDS {
            return;
        }
        if !self.visited.insert(dict_id(field)) {
            return;
        }
        let name = qualified_name(field, prefix);
        // A dict whose kids are fields of their own is a container: it names no field itself, but
        // its name is part of its kids'.
        if !is_container(field) {
            self.found.push((field.clone(), name.clone()));
        }
        let Some(kids) = field.get::<Array<'_>>(b"Kids") else {
            return;
        };
        for kid in kids.iter::<Object<'_>>() {
            let Object::Dict(kid) = kid else {
                continue;
            };
            // A kid that names itself with `/T` is a field of its own; one without is a widget of
            // this field, whose value is compared here.
            if !kid.contains_key(b"T") {
                continue;
            }
            self.visit(&kid, &name, depth + 1);
            if self.found.len() >= MAX_FIELDS {
                return;
            }
        }
    }
}

/// The widgets a field draws: its kids that name no field of their own, or the field itself when it
/// is a widget and has no kids.
fn field_widgets<'a>(field: &Dict<'a>) -> Vec<Dict<'a>> {
    let mut widgets = Vec::new();
    if let Some(kids) = field.get::<Array<'a>>(b"Kids") {
        for kid in kids.iter::<Object<'a>>() {
            if let Object::Dict(kid) = kid
                && !kid.contains_key(b"T")
            {
                widgets.push(kid);
            }
        }
    }
    if widgets.is_empty()
        && (field.contains_key(b"Rect")
            || field.get::<Name<'a>>(b"Subtype").as_deref() == Some(b"Widget"))
    {
        widgets.push(field.clone());
    }
    widgets
}

/// The appearance state a button shows: the `/AS` of its widget, or, for a radio group whose
/// widgets each state one, the state that is not `Off`. A field of another kind shows no state.
fn field_state(field: &Dict<'_>, kind: &str) -> Value {
    fn state_of(widget: &Dict<'_>) -> Option<String> {
        widget
            .get::<Name<'_>>(b"AS")
            .map(|state| String::from_utf8_lossy(&state).into_owned())
    }
    match kind {
        "checkbox" => field_widgets(field)
            .first()
            .and_then(state_of)
            .map_or(Value::Null, Value::from),
        "radio" => {
            let widgets = field_widgets(field);
            for widget in &widgets {
                if let Some(state) = state_of(widget)
                    && state != "Off"
                {
                    return Value::from(state);
                }
            }
            if widgets.is_empty() {
                Value::Null
            } else {
                Value::from("Off")
            }
        }
        _ => Value::Null,
    }
}

/// The strings a field's appearance draws, widget by widget, in widget order.
fn field_appearance(
    field: &Dict<'_>,
    cache: &mut ToUnicodeCache,
    budget: &mut Decoded,
) -> Result<Value> {
    let mut out = Vec::new();
    for widget in field_widgets(field) {
        let Some(stream) = normal_appearance(&widget) else {
            continue;
        };
        walk_form(&stream, &mut out, cache, budget, 0)?;
    }
    Ok(Value::Array(out))
}

/// The normal appearance stream a widget shows: its `/AP /N` when that is a stream, or the state
/// its `/AS` names when it is a dictionary of states.
fn normal_appearance<'a>(widget: &Dict<'a>) -> Option<Stream<'a>> {
    let appearances = widget.get::<Dict<'a>>(b"AP")?;
    match appearances.get::<Object<'a>>(b"N")? {
        Object::Stream(stream) => Some(stream),
        Object::Dict(states) => {
            let state = widget.get::<Name<'a>>(b"AS");
            let key: &[u8] = state.as_deref().unwrap_or(b"Off");
            states.get::<Stream<'a>>(key)
        }
        _ => None,
    }
}

/// What tells one field dictionary from another within one document: the bytes it was parsed
/// from. Two dictionaries written directly into one array are two slices, while a reference
/// resolves to the one slice it points at.
fn dict_id(dict: &Dict<'_>) -> (usize, usize) {
    let data = dict.data();
    (data.as_ptr().addr(), data.len())
}

/// A field's fully qualified name: its own `/T` under the prefix its ancestors established. A
/// field with no `/T` of its own contributes nothing but its kids still count.
fn qualified_name(field: &Dict<'_>, prefix: &str) -> String {
    let own = field
        .get::<PdfString>(b"T")
        .map(|text| pdf_text(text.as_bytes()))
        .unwrap_or_default();
    match (prefix.is_empty(), own.is_empty()) {
        (true, _) => own,
        (false, true) => prefix.to_string(),
        (false, false) => format!("{prefix}.{own}"),
    }
}

/// Whether a field is a container rather than a field: a dict whose `/Kids` are fields of their
/// own.
fn is_container(field: &Dict<'_>) -> bool {
    field.get::<Array<'_>>(b"Kids").is_some_and(|kids| {
        kids.iter::<Object<'_>>()
            .any(|kid| matches!(kid, Object::Dict(kid) if kid.contains_key(b"T")))
    })
}

/// A field's kind, from its `/FT` and `/Ff` inherited along the `/Parent` chain.
fn field_kind(field: &Dict<'_>) -> &'static str {
    let Some(kind) = inherited(field, b"FT").and_then(Object::into_name) else {
        return "unknown";
    };
    match kind.as_str() {
        "Tx" => "text",
        "Ch" => "choice",
        "Sig" => "signature",
        "Btn" => {
            let flags = field_flags(field);
            if flags & FIELD_FLAG_RADIO != 0 {
                "radio"
            } else if flags & FIELD_FLAG_PUSH_BUTTON != 0 {
                "button"
            } else {
                "checkbox"
            }
        }
        _ => "unknown",
    }
}

/// A field's `/Ff`, inherited; a field that states none has no flag set.
fn field_flags(field: &Dict<'_>) -> i32 {
    inherited(field, b"Ff")
        .and_then(Object::into_i32)
        .unwrap_or(0)
}

/// A field's `/V`: a string, a name (a button's state), or the options a multi-select chose.
/// `null` when the field holds no value at all.
fn field_value(field: &Dict<'_>) -> Value {
    match inherited(field, b"V") {
        Some(Object::String(text)) => Value::from(pdf_text(text.as_bytes())),
        Some(Object::Name(name)) => Value::from(String::from_utf8_lossy(&name).into_owned()),
        // A choice that selects more than one option holds them as an array; the model spells
        // them in document order, so two selections differ.
        Some(Object::Array(items)) => Value::from(
            items
                .iter::<Object<'_>>()
                .filter_map(|item| match item {
                    Object::String(text) => Some(pdf_text(text.as_bytes())),
                    Object::Name(name) => Some(String::from_utf8_lossy(&name).into_owned()),
                    _ => None,
                })
                .collect::<Vec<_>>()
                .join(", "),
        ),
        _ => Value::Null,
    }
}

/// One `/Opt` entry's text, when the entry is a string.
fn option_text(object: Object<'_>) -> Option<String> {
    match object {
        Object::String(text) => Some(pdf_text(text.as_bytes())),
        _ => None,
    }
}

/// The options a field offers: a radio group's or a checkbox's export values, and a choice's — or a
/// push button's — display names.
fn field_options(field: &Dict<'_>, kind: &str) -> Value {
    let mut options = Vec::new();
    // The kinds read the same entry differently: a radio group's or a checkbox's (`PDFAcroButton`)
    // `/Opt` is its export values, which are the top-level strings alone, so a pair names none; for
    // a choice field — and for a push button, which the model does not single out — it is the
    // display names, which a pair's second entry states.
    let button = matches!(kind, "radio" | "checkbox");
    match inherited(field, b"Opt") {
        Some(Object::String(text)) => options.push(pdf_text(text.as_bytes())),
        Some(Object::Array(items)) => {
            for item in items.iter::<Object<'_>>() {
                if options.len() >= MAX_FIELD_OPTIONS {
                    break;
                }
                match item {
                    Object::String(text) => options.push(pdf_text(text.as_bytes())),
                    // An `[export display]` pair: the display name is its second entry, and its
                    // first when the pair states one entry — the way both engines read it.
                    Object::Array(pair) if !button => {
                        let mut entries = pair.iter::<Object<'_>>();
                        let first = entries.next().and_then(option_text);
                        let second = entries.next().and_then(option_text);
                        if let Some(display) = second.or(first) {
                            options.push(display);
                        }
                    }
                    _ => {}
                }
            }
        }
        _ => {}
    }
    Value::Array(options.into_iter().map(Value::from).collect())
}

/// An inheritable field entry, read along the `/Parent` chain — where a widget keeps what its
/// field states.
fn inherited<'a>(field: &Dict<'a>, key: &[u8]) -> Option<Object<'a>> {
    let mut current = field.clone();
    for _ in 0..MAX_FIELD_DEPTH {
        if let Some(value) = current.get::<Object<'a>>(key) {
            return Some(value);
        }
        current = current.get::<Dict<'a>>(b"Parent")?;
    }
    None
}

/// The names of the document's embedded files, from the `/EmbeddedFiles` name tree.
fn attachment_names(pdf: &Pdf) -> Vec<Value> {
    let mut names = Vec::new();
    if let Some(tree) = catalog(pdf)
        .and_then(|catalog| catalog.get::<Dict<'_>>(b"Names"))
        .and_then(|names| names.get::<Dict<'_>>(b"EmbeddedFiles"))
    {
        collect_names(&tree, &mut names, 0);
    }
    names
}

/// Every name in one name tree: the `/Names` array's alternating (name, value) pairs, then any
/// `/Kids` subtree. Best-effort: a node that states neither contributes nothing.
fn collect_names(node: &Dict<'_>, names: &mut Vec<Value>, depth: usize) {
    if depth > MAX_NAME_TREE_DEPTH || names.len() >= MAX_ATTACHMENTS {
        return;
    }
    if let Some(pairs) = node.get::<Array<'_>>(b"Names") {
        let mut items = pairs.flex_iter();
        while let Some(name) = items.next::<Object<'_>>() {
            if names.len() >= MAX_ATTACHMENTS {
                return;
            }
            if let Object::String(text) = name {
                names.push(Value::from(pdf_text(text.as_bytes())));
            }
            // The value after each name is the file specification, which holds the file itself.
            let _ = items.next::<Object<'_>>();
        }
    }
    if let Some(kids) = node.get::<Array<'_>>(b"Kids") {
        for kid in kids.iter::<Object<'_>>() {
            if let Object::Dict(kid) = kid {
                collect_names(&kid, names, depth + 1);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::corpus::build::PdfObjects;

    #[test]
    fn a_document_over_the_page_limit_is_refused() {
        charge_pages(MAX_PAGES).expect("the bound itself is allowed");
        let error = charge_pages(MAX_PAGES + 1).expect_err("one page over the bound is refused");
        assert!(error.to_string().contains("pages"), "{error}");
    }

    #[test]
    fn a_stream_over_the_operator_limit_is_refused() {
        let mut count = 0;
        for _ in 0..MAX_STREAM_OPERATORS {
            charge_operators(&mut count).expect("the bound itself is allowed");
        }
        let error =
            charge_operators(&mut count).expect_err("one operator over the bound is refused");
        assert!(error.to_string().contains("operators"), "{error}");
    }

    #[test]
    fn an_image_over_the_decode_limit_is_refused() {
        // Four bytes per pixel: the bound's own side in each dimension, then one pixel more.
        let side = u32::try_from((MAX_IMAGE_BYTES / 4).isqrt()).expect("the bound's side fits");
        let mut budget = Decoded::default();
        budget
            .image_area(side, side)
            .expect("the bound itself is allowed");
        let error = budget
            .image_area(side + 1, side)
            .expect_err("one row over the per-image bound is refused");
        assert!(error.to_string().contains("decode limit"), "{error}");
    }

    #[test]
    fn an_image_claiming_the_whole_u32_range_is_refused_rather_than_wrapping() {
        // (2^32-1)^2 * 4 is past `u64`: the charge must saturate and refuse, not wrap to zero and
        // let the decode the bound exists to prevent happen.
        let mut budget = Decoded::default();
        let error = budget
            .image_area(u32::MAX, u32::MAX)
            .expect_err("a crafted size is refused");
        assert!(error.to_string().contains("decode limit"), "{error}");
    }

    #[test]
    fn a_document_over_the_decoded_total_is_refused() {
        let side = u32::try_from((MAX_IMAGE_BYTES / 4).isqrt()).expect("the bound's side fits");
        let image = u64::from(side) * u64::from(side) * 4;
        let mut budget = Decoded::default();
        let mut charged = 0;
        while budget.total + image <= MAX_DECODED_BYTES {
            budget
                .image_area(side, side)
                .expect("each image is under the per-image bound");
            charged += 1;
        }
        let error = budget
            .image_area(side, side)
            .expect_err("the total's own bound is not exceeded by an image under it");
        assert!(error.to_string().contains("decoded streams"), "{error}");
        assert!(charged > 1, "the total admits more than one image");
    }

    #[test]
    fn one_stream_object_is_charged_once_however_often_it_is_painted() {
        // A page painting one small image many times is one image in memory, so it is one charge;
        // an object with no identifier is charged every time, because it cannot be told apart.
        let mut budget = Decoded::default();
        let image = ObjectIdentifier::new(7, 0);
        assert!(budget.first(Some(image)), "the first paint is charged");
        assert!(!budget.first(Some(image)), "the second paint is free");
        assert!(budget.first(None), "an inline image is charged");
        assert!(budget.first(None), "and so is the next one");
    }

    // ── the charge paths ──────────────────────────────────────────────────────────────────────
    //
    // Three of the four places the model decodes a stream — a page's content, a form XObject and a
    // font's `/ToUnicode` map; images have their own test above — are what bound it. Each is driven
    // here on a document that really holds the stream, with a budget already spent, so the charge,
    // not the decode, is what refuses.

    /// A one-page PDF: `page` writes the page's own dictionary, with `{parent}` standing for its
    /// parent, and every object it names.
    fn one_page(page: impl FnOnce(&mut PdfObjects) -> String) -> Vec<u8> {
        let mut pdf = PdfObjects::default();
        let catalog = pdf.reserve();
        let pages = pdf.reserve();
        let page_id = pdf.reserve();
        let page = page(&mut pdf).replace("{parent}", &pages);
        pdf.set(
            &pages,
            format!("<< /Type /Pages /Kids [{page_id}] /Count 1 >>"),
        );
        pdf.set(&page_id, page);
        pdf.set(&catalog, format!("<< /Type /Catalog /Pages {pages} >>"));
        pdf.finish(&catalog, "")
    }

    /// The first page of `bytes`, parsed.
    fn first_page(bytes: Vec<u8>) -> Pdf {
        let pdf = Pdf::new(bytes).expect("the fixture is a readable PDF");
        assert_eq!(pdf.pages().len(), 1, "the fixture is one page");
        pdf
    }

    /// A budget whose whole total is already spent, so any further charge refuses.
    fn spent() -> Decoded {
        Decoded {
            total: MAX_DECODED_BYTES,
            charged: HashSet::new(),
        }
    }

    #[test]
    fn one_content_stream_is_charged_its_decoded_length() {
        let pdf = first_page(one_page(|pdf| {
            let content = pdf.stream("", b"BT /F1 12 Tf (Hello) Tj ET");
            format!(
                "<< /Type /Page /Parent {{parent}} /MediaBox [0 0 200 100] /Contents {content} >>"
            )
        }));
        let page = &pdf.pages()[0];
        let mut budget = Decoded::default();
        charge_content(page, &mut budget).expect("one small stream is under the bound");
        assert_eq!(budget.total, 26, "the content stream's own bytes");
        let error = charge_content(page, &mut spent()).expect_err("a spent total refuses it");
        assert!(error.to_string().contains("decoded streams"), "{error}");
    }

    #[test]
    fn an_array_of_content_streams_is_charged_with_the_joins_between_them() {
        // The model charges what the operator walk will read: the format joins an array's streams
        // with a space each, and reading them any other way would be a second decode.
        let pdf = first_page(one_page(|pdf| {
            let first = pdf.stream("", b"q");
            let second = pdf.stream("", b"Q");
            format!(
                "<< /Type /Page /Parent {{parent}} /MediaBox [0 0 200 100] \
                 /Contents [{first} {second}] >>"
            )
        }));
        let page = &pdf.pages()[0];
        let content = page.page_stream().expect("the fixture's page has content");
        let mut budget = Decoded::default();
        charge_content(page, &mut budget).expect("two small streams are under the bound");
        assert_eq!(
            budget.total, 4,
            "two one-byte streams and the space after each"
        );
        assert!(
            std::ptr::eq(content, page.page_stream().expect("the content is cached")),
            "the charge and the operator walk read one decode, not two"
        );
        let error = charge_content(page, &mut spent()).expect_err("a spent total refuses them");
        assert!(error.to_string().contains("decoded streams"), "{error}");
    }

    #[test]
    fn a_form_xobject_is_charged_its_decoded_content() {
        let pdf = first_page(one_page(|pdf| {
            let form = pdf.stream(
                "/Type /XObject /Subtype /Form /BBox [0 0 10 10]",
                b"0 0 m S",
            );
            let content = pdf.stream("", b"/Fm1 Do");
            format!(
                "<< /Type /Page /Parent {{parent}} /MediaBox [0 0 200 100] /Contents {content} \
                 /Resources << /XObject << /Fm1 {form} >> >> >>"
            )
        }));
        let page = &pdf.pages()[0];
        let form = page
            .raw()
            .get::<Dict<'_>>(b"Resources")
            .and_then(|resources| resources.get::<Dict<'_>>(b"XObject"))
            .and_then(|xobjects| xobjects.get::<Stream<'_>>(b"Fm1"))
            .expect("the fixture's page names its form");
        let mut budget = Decoded::default();
        let mut out = Vec::new();
        let mut cache = HashMap::new();
        walk_form(&form, &mut out, &mut cache, &mut budget, 0)
            .expect("a small form is under the bound");
        assert_eq!(budget.total, 7, "the form's decoded content");
        assert!(
            out.is_empty(),
            "a form that draws no text states none to the model"
        );
        let error = walk_form(&form, &mut out, &mut cache, &mut spent(), 0)
            .expect_err("a spent total refuses the form");
        assert!(error.to_string().contains("decoded streams"), "{error}");
    }

    #[test]
    fn a_to_unicode_map_is_charged_once_and_refused_past_the_total() {
        let pdf = first_page(one_page(|pdf| {
            let unicode = pdf.stream("", CMAP);
            let font = pdf.push(format!(
                "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /ToUnicode {unicode} >>"
            ));
            let content = pdf.stream("", b"BT /F1 12 Tf (A) Tj ET");
            format!(
                "<< /Type /Page /Parent {{parent}} /MediaBox [0 0 200 100] /Contents {content} \
                 /Resources << /Font << /F1 {font} >> >> >>"
            )
        }));
        let page = &pdf.pages()[0];
        let font = page
            .raw()
            .get::<Dict<'_>>(b"Resources")
            .and_then(|resources| resources.get::<Dict<'_>>(b"Font"))
            .and_then(|fonts| fonts.get::<Dict<'_>>(b"F1"))
            .expect("the fixture's page names its font");
        let mut cache = ToUnicodeCache::new();
        let mut budget = Decoded::default();
        let map = to_unicode_map(&font, &mut cache, &mut budget).expect("the map parses");
        assert!(map.is_some(), "the font names a readable /ToUnicode map");
        assert_eq!(budget.total, CMAP.len() as u64, "the map's own bytes");
        // The page's later runs read the cached map, so the document is charged once for it.
        to_unicode_map(&font, &mut cache, &mut budget).expect("the cached map is read again");
        assert_eq!(
            budget.total,
            CMAP.len() as u64,
            "a cached map is not charged twice"
        );
        let error = to_unicode_map(&font, &mut ToUnicodeCache::new(), &mut spent())
            .expect_err("a spent total refuses the map");
        assert!(error.to_string().contains("decoded streams"), "{error}");
    }

    /// A minimal `/ToUnicode` map: the two-byte code space a simple font states, and one character.
    const CMAP: &[u8] = b"/CIDInit /ProcSet findresource begin\n\
12 dict begin\n\
begincmap\n\
/CMapName /Test def\n\
1 begincodespacerange\n\
<0000> <FFFF>\n\
endcodespacerange\n\
1 beginbfchar\n\
<0041> <0042>\n\
endbfchar\n\
endcmap\n\
CMapName currentdict /CMap defineresource pop\n\
end\n\
end\n";
}

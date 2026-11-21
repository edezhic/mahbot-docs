//! A damaged PDF rewritten twice: the crate's arm and the JavaScript kit, compared where the recorded
//! model cannot look.
//!
//! The recorded model cannot witness what this test exists for. It compares no bytes, normalises object
//! bodies and ignores an object no page reaches, so a produced case passes whether the engine carries
//! a damaged object's bytes through or writes an empty placeholder in its place — and the same is true
//! of a reference a copied page reaches that the source does not hold. That is a stated limit of the
//! comparison, recorded in `parity/src/canonical/pdf.rs` and in the engine's own documentation, and
//! this test is the out-of-model witness it points at: it runs one request through both writers and
//! compares what each side wrote.
//!
//! Every property compared here is invisible to the model — one test may compare more than one of
//! them:
//!
//! - an object whose body does not parse is written back as the bytes it stated, verbatim, by both
//!   sides — so the produced file is as damaged as its input and no more;
//! - a dictionary that closes while its last key states no value is such a body, never one the engine
//!   repairs;
//! - a reference a copied page reaches that the source does not hold is left dangling by both sides,
//!   rather than resolved to nothing or to a made-up object;
//! - a page whose `/Contents` is a dictionary is carried by a merge as the page stated it — neither
//!   dropped, nor turned into a stream, nor resolved away;
//! - a container whose every entry states one offset is read and produced by both sides;
//! - a reference inside a body the walk carried is reported by the crate, whose object moved, and kept
//!   by the reference, which states the object where the file did;
//! - an object stated at number zero is dropped by the crate, which cannot name it, and kept by the
//!   reference where the file stated it;
//! - one number stated at two generations is reported by the crate, whose index names one body per
//!   number, and kept by the reference;
//! - two object numbers past what a `u64` holds that the reference keys apart are kept by both sides;
//! - one number stated in two runs of digits that round to one double leaves one body on both sides;
//! - a body the walk carried in place of a number is what both sides write, and the body it replaced
//!   is in neither file;
//! - a reference to `4294967295 65535` — the id the walk's first marker stands at, named by a file
//!   that states an object past the model first — is left naming nothing by both sides, at the number
//!   the file used;
//! - a page-tree entry stated as a reference that names a reference is refused by both sides;
//! - a box entry stated as a reference that names a reference faults on both sides;
//! - a form reached through a reference that names a reference faults on both sides;
//! - a page's resources reached through a reference that names a reference fault on both sides;
//! - a field's `/Parent` stated as a reference that names a reference is declined by both sides;
//! - a form's `/Fields` entry stated as a reference that names a reference is declined by both sides;
//! - a `/Fields` or a `/Kids` entry that names an array of its own is the same list one step on, so
//!   both sides fill the field it holds;
//! - a kid stated where it stands is no field of a container's on either side, and a kid that is not a
//!   dictionary is a fault on both, while a field that states an empty `/Kids` array draws no appearance
//!   on either side;
//! - a field whose `/FT` is null under a parent that states one, and a field whose `/T` names nothing,
//!   are read the way the reference's inheritance and `lookupMaybe` read them: both sides fill under the
//!   name the chain builds;
//! - a text field the request does not fill keeps the appearance the file states, so its `/Ff` — a
//!   reference to a reference — is never read and both sides produce;
//! - a `/Ff` stated as a reference that names nothing is no flags at all, so both sides fill the choice
//!   field it belongs to;
//! - a `/Parent` stated as a dictionary in place is followed for the entries a field inherits while it
//!   is built, and for no name, so both sides fill the field under its own `/T`;
//! - a `/Kids` that is not an array is a fault on both sides where the widget list is read, and a
//!   malformed kid beside a request that names no field is the usage refusal both sides answer first;
//! - a `/Ff` is read with JavaScript's 32-bit `&`: a negative real sets every bit, so a choice field is
//!   a combo box and a button a push button, and a real past 2^32 keeps no bit the kind reads, so the
//!   field is a list box — both sides producing or refusing alike;
//! - a `/Ff` real past what the crate's 32-bit float states exactly is read differently, the reference
//!   making `4294967295.5` a combo box and producing where the crate makes it a list box and refuses,
//!   and a real `/MaxLen` bounds the reference where the crate writes the value;
//! - a field whose kid list or parent link returns to itself dies with a stack overflow in the
//!   reference and is answered by the crate;
//! - a box entry stated as a null is "no crop" on both sides, so the page falls back to the media box
//!   rather than faulting on the entry;
//! - a container whose `/N` is not a number is carried as the bytes it states by both sides, with
//!   nothing the entries name placed — not even over an object the document already holds;
//! - a file whose streams never end is turned by both sides, and the reference's own output of it is a
//!   file the crate reads and turns again.
//!
//! No test skips. [`pair`] fails when the runtime the kit needs (`bun` on `PATH`, or `PARITY_BUN`) is
//! absent, and that failure is left to panic: a comparison that did not run is not a comparison that
//! passed.

mod common;

use common::{both_sides, carried_out, fixture, pair, produced, sentence};
use document_parity::reference::CrateWriter;
use hayro::hayro_syntax::Pdf;
use hayro::hayro_syntax::object::MaybeRef;
use hayro::hayro_syntax::object::Object;
use hayro::hayro_syntax::object::dict::Dict;
use serde_json::json;

/// The bytes the broken object states, as the file states them: the object's header and `endobj` are
/// the file's, and this is what lies between them.
const BROKEN_BODY: &str = "<< /Type /Font /Subtype /Type1 /BaseFont !! broken\n";

#[test]
fn a_body_that_does_not_parse_is_written_back_verbatim() {
    let (root, reference) = pair();
    let engine = CrateWriter;
    let input = fixture(
        root.path(),
        "body-unparsable.pdf",
        document_engine::test_support::build::pdf_body_unparsable(),
    );
    let request = json!({
        "op": "pdf_rotate",
        "input": input.to_string_lossy(),
        "degrees": 90,
        "pages": "all",
    });
    let ours = produced(&engine, root.path(), "crate", &request);
    let theirs = produced(&reference, root.path(), "reference", &request);
    let broken = BROKEN_BODY.as_bytes();
    assert!(
        ours.windows(broken.len()).any(|window| window == broken),
        "the crate did not write the damaged object's bytes"
    );
    assert!(
        theirs.windows(broken.len()).any(|window| window == broken),
        "the reference did not write the damaged object's bytes"
    );
}

#[test]
fn a_dictionary_whose_last_key_states_no_value_is_written_back_verbatim() {
    // The reference's parser throws on a dictionary that closes with a key still waiting for its value,
    // so the object is kept as the bytes it stated. A walk that repaired it would write a dictionary
    // the file does not state, which is more than the input held.
    let (root, reference) = pair();
    let engine = CrateWriter;
    let input = fixture(
        root.path(),
        "dictionary-unparsable.pdf",
        document_engine::test_support::build::pdf_dictionary_unparsable(),
    );
    let request = json!({
        "op": "pdf_rotate",
        "input": input.to_string_lossy(),
        "degrees": 90,
        "pages": "all",
    });
    let ours = produced(&engine, root.path(), "crate", &request);
    let theirs = produced(&reference, root.path(), "reference", &request);
    let stated = b"<< /Type /Font /Subtype /Type1 /BaseFont >>";
    assert!(
        ours.windows(stated.len()).any(|window| window == stated),
        "the crate repaired the damaged dictionary"
    );
    assert!(
        theirs.windows(stated.len()).any(|window| window == stated),
        "the reference did not write the damaged dictionary's bytes"
    );
}

#[test]
fn a_reference_the_source_does_not_hold_is_left_dangling_by_both_sides() {
    let (root, reference) = pair();
    let engine = CrateWriter;
    let input = fixture(
        root.path(),
        "dangling-reference.pdf",
        document_engine::test_support::build::pdf_dangling_reference(),
    );
    let request = json!({
        "op": "pdf_merge",
        "inputs": [input.to_string_lossy()],
    });
    let ours = produced(&engine, root.path(), "crate", &request);
    let theirs = produced(&reference, root.path(), "reference", &request);
    // A page of the merged file states a font entry that does not resolve: the entry is there, so the
    // name was not dropped, and it names nothing, so nothing was invented for it.
    for (name, bytes) in [("the crate", &ours), ("the reference", &theirs)] {
        let pdf = Pdf::new(bytes.clone())
            .unwrap_or_else(|error| panic!("{name}: the produced file does not parse ({error:?})"));
        let pages = pdf.pages();
        assert_eq!(pages.len(), 2, "{name}: both pages came across");
        let resources: Dict = pages[0]
            .raw()
            .get(b"Resources")
            .unwrap_or_else(|| panic!("{name}: the page states no resources"));
        let fonts: Dict = resources
            .get(b"Font")
            .unwrap_or_else(|| panic!("{name}: the page states no fonts"));
        assert!(
            matches!(fonts.get_raw::<Object>(b"F1"), Some(MaybeRef::Ref(_))),
            "{name} did not leave the font entry the page states dangling rather than resolved to nothing"
        );
        assert!(
            fonts.get::<Dict>(b"F1").is_none(),
            "{name} resolved the reference the source does not hold"
        );
    }
}

/// A merge of a page whose `/Contents` is a dictionary. The canonical model's page-stream reader
/// follows such a dictionary to the next `stream` keyword in the file, so what a merge of this shape
/// writes is deliberately not a corpus case: the model cannot see the entry, and this test is what
/// witnesses that the shape survives — the page states the same dictionary afterwards.
#[test]
fn a_page_whose_contents_is_a_dictionary_is_carried_by_a_merge() {
    let (root, reference) = pair();
    let engine = CrateWriter;
    let input = fixture(
        root.path(),
        "contents-dictionary.pdf",
        document_engine::test_support::build::pdf_contents_dictionary(),
    );
    let request = json!({
        "op": "pdf_merge",
        "inputs": [input.to_string_lossy()],
    });
    let ours = produced(&engine, root.path(), "crate", &request);
    let theirs = produced(&reference, root.path(), "reference", &request);
    for (name, bytes) in [("the crate", &ours), ("the reference", &theirs)] {
        let pdf = Pdf::new(bytes.clone())
            .unwrap_or_else(|error| panic!("{name}: the produced file does not parse ({error:?})"));
        let pages = pdf.pages();
        assert_eq!(pages.len(), 1, "{name}: the page came across");
        let page = pages[0].raw();
        // A merge renumbers what it carries, so the page may name the dictionary indirectly; what must
        // not happen is the entry vanishing, becoming a stream, or resolving to nothing.
        let contents = page
            .get_raw::<Object>(b"Contents")
            .unwrap_or_else(|| panic!("{name}: the page states no /Contents"));
        assert!(
            !matches!(contents, MaybeRef::NotRef(Object::Stream(_))),
            "{name}: the dictionary at /Contents was turned into a stream"
        );
        let dictionary: Dict = page
            .get(b"Contents")
            .unwrap_or_else(|| panic!("{name}: the page states no dictionary at /Contents"));
        assert!(
            dictionary.get::<Object>(b"A").is_some(),
            "{name}: the dictionary at /Contents is not the one the page stated"
        );
    }
}

#[test]
fn a_container_whose_entries_state_one_offset_is_produced_by_both_sides() {
    // Every entry of the object stream states the same offset. The reference parses the entry per
    // offset the file states and produces, so refusing this file would refuse one it rewrites.
    let (root, reference) = pair();
    let engine = CrateWriter;
    let input = fixture(
        root.path(),
        "one-offset.pdf",
        document_engine::test_support::build::pdf_object_stream_one_offset(),
    );
    let request = json!({
        "op": "pdf_rotate",
        "input": input.to_string_lossy(),
        "degrees": 90,
        "pages": "all",
    });
    let (said, ours) = carried_out(&engine, root.path(), "crate", &request);
    let theirs = produced(&reference, root.path(), "reference", &request);
    assert!(said.losses.is_empty(), "the crate lost nothing: {said:?}");
    for (name, bytes) in [("the crate", &ours), ("the reference", &theirs)] {
        let pdf = Pdf::new(bytes.clone())
            .unwrap_or_else(|error| panic!("{name}: the produced file does not parse ({error:?})"));
        let pages = pdf.pages();
        assert_eq!(pages.len(), 1, "{name}: one page came across");
        let rotate: i64 = pages[0]
            .raw()
            .get(b"Rotate")
            .unwrap_or_else(|| panic!("{name}: the page states no /Rotate"));
        assert_eq!(rotate, 90, "{name}: the page was not turned");
    }
}

#[test]
fn a_carried_body_reference_is_reported_by_the_crate_and_kept_by_the_reference() {
    // The object at `5000000000 0 obj` is given a number of this document's own, and the body the walk
    // carried is written back verbatim, so the reference inside it names nothing and the answer says
    // so. The reference keeps the object at the number the file stated, compressed into an object
    // stream, and names it in the cross-reference index it writes — measured, its file carries no
    // `5000000000 0 obj` body either, and hayro does not parse it.
    let (root, reference) = pair();
    let engine = CrateWriter;
    let input = fixture(
        root.path(),
        "carried-reference.pdf",
        document_engine::test_support::build::pdf_carried_reference_to_a_placed_object(),
    );
    let request = json!({
        "op": "pdf_rotate",
        "input": input.to_string_lossy(),
        "degrees": 90,
        "pages": "all",
    });
    let (said, ours) = carried_out(&engine, root.path(), "crate", &request);
    let (theirs_said, theirs) = carried_out(&reference, root.path(), "reference", &request);
    assert_eq!(
        said.losses,
        vec![sentence("pdf_page_references_lost", "references", 1)],
        "the crate reports the reference that lost its object"
    );
    assert!(
        theirs_said.losses.is_empty(),
        "the reference reports nothing: {theirs_said:?}"
    );
    assert!(
        ours.windows(14).any(|window| window == b"5000000000 0 R"),
        "the crate did not write the carried body as the file stated it"
    );
    // The reference's index states the object where the file did: a run of three entries starting at
    // `5000000000`, which is the object itself, the container and the index.
    assert!(
        theirs.windows(12).any(|window| window == b"5000000000 3"),
        "the reference did not keep the object at the number the file states"
    );
}

#[test]
fn an_object_at_number_zero_is_dropped_by_the_crate_and_kept_by_the_reference() {
    // The index states no entry at number zero, so the crate leaves the object out of a produced file
    // a reader can follow and reports it. The reference writes it where the file stated it.
    let (root, reference) = pair();
    let engine = CrateWriter;
    let input = fixture(
        root.path(),
        "object-at-zero.pdf",
        document_engine::test_support::build::pdf_object_at_number_zero(),
    );
    let request = json!({
        "op": "pdf_rotate",
        "input": input.to_string_lossy(),
        "degrees": 90,
        "pages": "all",
    });
    let (said, ours) = carried_out(&engine, root.path(), "crate", &request);
    let (theirs_said, theirs) = carried_out(&reference, root.path(), "reference", &request);
    assert_eq!(
        said.losses,
        vec![sentence("pdf_page_objects_lost", "objects", 1)],
        "the crate reports the object it dropped"
    );
    assert!(
        !ours.windows(9).any(|window| window == b"\n0 1 obj\n"),
        "the crate wrote an object at number zero"
    );
    assert!(
        theirs.windows(9).any(|window| window == b"\n0 1 obj\n"),
        "the reference did not keep the object at number zero"
    );
    assert!(
        theirs_said.losses.is_empty(),
        "the reference reports nothing: {theirs_said:?}"
    );
}

#[test]
fn two_generations_of_one_number_are_reported_by_the_crate() {
    // An index states one entry per number, so one of the two bodies is unreachable in a produced
    // file. The crate names the body it cannot carry; the reference keeps the one it reads last and
    // reports nothing.
    let (root, reference) = pair();
    let engine = CrateWriter;
    let input = fixture(
        root.path(),
        "two-generations.pdf",
        document_engine::test_support::build::pdf_two_generations_of_one_number(),
    );
    let request = json!({
        "op": "pdf_rotate",
        "input": input.to_string_lossy(),
        "degrees": 90,
        "pages": "all",
    });
    let (said, ours) = carried_out(&engine, root.path(), "crate", &request);
    let (theirs_said, theirs) = carried_out(&reference, root.path(), "reference", &request);
    assert_eq!(
        said.losses,
        vec![sentence("pdf_page_objects_lost", "objects", 1)],
        "the crate reports the body the index cannot name"
    );
    assert!(
        theirs_said.losses.is_empty(),
        "the reference reports nothing: {theirs_said:?}"
    );
    for (name, bytes) in [("the crate", &ours), ("the reference", &theirs)] {
        let pdf = Pdf::new(bytes.clone())
            .unwrap_or_else(|error| panic!("{name}: the produced file does not parse ({error:?})"));
        assert_eq!(pdf.pages().len(), 2, "{name}: both pages came across");
    }
}

#[test]
fn a_chained_page_tree_entry_is_refused_by_both_sides() {
    // A typed lookup takes one step, finds a reference rather than the array or node it needs, and the
    // load refuses the file — on both sides, with nothing written.
    let (root, reference) = pair();
    for (name, bytes) in [
        (
            "kids-by-reference",
            document_engine::test_support::build::pdf_page_tree_kids_by_reference(),
        ),
        (
            "root-by-reference",
            document_engine::test_support::build::pdf_page_tree_root_by_reference(),
        ),
    ] {
        let input = fixture(root.path(), &format!("{name}.pdf"), bytes);
        let request = json!({
            "op": "pdf_rotate",
            "input": input.to_string_lossy(),
            "degrees": 90,
            "pages": "all",
        });
        for side in both_sides(&reference, root.path(), name, &request) {
            let who = side.who;
            let failure = side.declined();
            assert_eq!(
                failure.class, "usage",
                "{who} {name}: the caller's file is the problem: {failure:?}"
            );
        }
    }
}

#[test]
fn a_chained_box_entry_faults_on_both_sides() {
    // `/MediaBox` is read with a lookup that is not a `lookupMaybe`: a reference that names a reference
    // is not four numbers, so the reference's own type check faults inside the resize, and the crate
    // faults as it does.
    let (root, reference) = pair();
    let input = fixture(
        root.path(),
        "media-box-by-reference.pdf",
        document_engine::test_support::build::pdf_media_box_by_reference(),
    );
    let request = json!({
        "op": "pdf_resize",
        "input": input.to_string_lossy(),
        "width": 400,
        "height": 500,
        "pages": "all",
    });
    for side in both_sides(&reference, root.path(), "box-by-reference", &request) {
        let who = side.who;
        let failure = side.declined();
        assert_eq!(
            failure.class, "internal",
            "{who}: the engine's own failure: {failure:?}"
        );
    }
}

#[test]
fn a_chained_acroform_reference_faults_on_both_sides() {
    // `/AcroForm` is read with a typed lookup that takes one step: a reference that names a reference
    // is not the form dictionary, so both sides fault before they fill anything.
    let (root, reference) = pair();
    let input = fixture(
        root.path(),
        "acroform-by-reference.pdf",
        document_engine::test_support::build::pdf_acroform_by_reference(),
    );
    let request = json!({
        "op": "pdf_form_fill",
        "input": input.to_string_lossy(),
        "values": { "name": "x" },
    });
    for side in both_sides(&reference, root.path(), "acroform-by-reference", &request) {
        let who = side.who;
        let failure = side.declined();
        assert_eq!(
            failure.class, "internal",
            "{who}: the engine's own failure: {failure:?}"
        );
    }
}

#[test]
fn a_chained_resources_reference_faults_on_both_sides() {
    // Drawing a mark reads the page's `/Resources` with a typed lookup that takes one step: what it
    // finds is a reference, not a dictionary, so the reference throws and the crate faults instead of
    // replacing entries it never read.
    let (root, reference) = pair();
    let input = fixture(
        root.path(),
        "resources-by-reference.pdf",
        document_engine::test_support::build::pdf_resources_by_reference(),
    );
    let request = json!({
        "op": "pdf_text",
        "input": input.to_string_lossy(),
        "text": "x",
        "pages": "all",
    });
    for side in both_sides(&reference, root.path(), "resources-by-reference", &request) {
        let who = side.who;
        let failure = side.declined();
        assert_eq!(
            failure.class, "internal",
            "{who}: the engine's own failure: {failure:?}"
        );
    }
}

#[test]
fn a_null_crop_box_falls_back_to_the_media_box_on_both_sides() {
    // `/CropBox` is read with a `lookupMaybe`, which answers "no crop" for a null the entry states:
    // both sides crop the page against its media box and produce the same box, where a fault on the
    // null would have produced nothing.
    let (root, reference) = pair();
    let engine = CrateWriter;
    let input = fixture(
        root.path(),
        "crop-box-null.pdf",
        document_engine::test_support::build::pdf_crop_box_null(),
    );
    let request = json!({
        "op": "pdf_crop",
        "input": input.to_string_lossy(),
        "area": { "x": 0.0, "y": 0.0, "width": 0.5, "height": 0.5 },
        "pages": "all",
    });
    let (said, ours) = carried_out(&engine, root.path(), "crate", &request);
    let theirs = produced(&reference, root.path(), "reference", &request);
    assert!(said.losses.is_empty(), "the crate lost nothing: {said:?}");
    let mut boxes = Vec::new();
    for (name, bytes) in [("the crate", &ours), ("the reference", &theirs)] {
        let pdf = Pdf::new(bytes.clone())
            .unwrap_or_else(|error| panic!("{name}: the produced file does not parse ({error:?})"));
        let pages = pdf.pages();
        assert_eq!(pages.len(), 1, "{name}: one page came across");
        let crop = pages[0]
            .raw()
            .get::<hayro::hayro_syntax::object::Array>(b"CropBox")
            .unwrap_or_else(|| panic!("{name}: the page states no /CropBox"))
            .iter::<f32>()
            .collect::<Vec<f32>>();
        assert!(
            crop[2] - crop[0] < 595.28 && crop[3] - crop[1] < 841.89,
            "{name}: the page was cropped rather than left whole: {crop:?}"
        );
        boxes.push(crop);
    }
    assert_eq!(boxes[0], boxes[1], "both sides crop to the same box");
}

#[test]
fn a_file_of_streams_that_never_end_is_turned_by_both_sides_and_read_again() {
    // A page followed by streams that state a length far past the file and never close: the reference
    // turns it, so the crate may not refuse it — and the reference's own output of that file is a
    // file the crate reads and turns again.
    let (root, reference) = pair();
    let engine = CrateWriter;
    let input = fixture(
        root.path(),
        "damaged-streams.pdf",
        document_engine::test_support::build::pdf_many_damaged_streams(40),
    );
    let request = json!({
        "op": "pdf_rotate",
        "input": input.to_string_lossy(),
        "degrees": 90,
        "pages": "all",
    });
    let (said, ours) = carried_out(&engine, root.path(), "crate", &request);
    assert!(said.losses.is_empty(), "the crate lost nothing: {said:?}");
    let theirs = produced(&reference, root.path(), "reference", &request);
    let again = fixture(root.path(), "reread.pdf", theirs);
    let second = json!({
        "op": "pdf_rotate",
        "input": again.to_string_lossy(),
        "degrees": 90,
        "pages": "all",
    });
    let (_, reread) = carried_out(&engine, root.path(), "crate-again", &second);
    let _ = produced(&reference, root.path(), "reference-again", &second);
    for (name, bytes) in [
        ("the crate", &ours),
        ("the crate over the reference's output", &reread),
    ] {
        let pdf = Pdf::new(bytes.clone())
            .unwrap_or_else(|error| panic!("{name}: the produced file does not parse ({error:?})"));
        assert_eq!(pdf.pages().len(), 2, "{name}: both pages came across");
    }
}

/// Whether `bytes` holds `marker` as it stands.
fn holds(bytes: &[u8], marker: &[u8]) -> bool {
    bytes.windows(marker.len()).any(|window| window == marker)
}

/// A produced file's bytes, then whatever its compressed streams hold: the reference packs the objects
/// it writes into an object stream when the document's numbers are large, so a body a produced file
/// keeps may be inside one rather than stated in the file's own bytes.
fn searchable(bytes: &[u8]) -> Vec<u8> {
    use std::io::Read as _;
    let mut all = bytes.to_vec();
    let mut rest = bytes;
    while let Some(at) = rest.windows(7).position(|window| window == b"stream\n") {
        let start = at + 7;
        let Some(end) = rest[start..]
            .windows(10)
            .position(|window| window == b"\nendstream")
        else {
            break;
        };
        let mut inflated = Vec::new();
        let _ =
            flate2::read::ZlibDecoder::new(&rest[start..start + end]).read_to_end(&mut inflated);
        all.extend_from_slice(&inflated);
        // Past the end marker, so the `stream` inside the word `endstream` is not taken for one.
        rest = &rest[start + end + b"\nendstream".len()..];
    }
    all
}

/// The number a produced file keys an object by, compared where the model cannot look: a run of digits
/// is a double to the reference, so two runs are two objects only when they round to two doubles, and
/// one object when they round to one.
#[test]
fn two_numbers_the_reference_keys_apart_are_kept_by_both_sides() {
    let (root, reference) = pair();
    let engine = CrateWriter;
    let input = fixture(
        root.path(),
        "numbers-keyed-apart.pdf",
        document_engine::test_support::build::pdf_two_numbers_the_reference_keys_apart(),
    );
    let request = json!({
        "op": "pdf_rotate",
        "input": input.to_string_lossy(),
        "degrees": 90,
        "pages": "all",
    });
    let (said, ours) = carried_out(&engine, root.path(), "crate", &request);
    let theirs = produced(&reference, root.path(), "reference", &request);
    assert!(said.losses.is_empty(), "the crate lost nothing: {said:?}");
    for (name, bytes) in [("the crate", &ours), ("the reference", &theirs)] {
        let bytes = searchable(bytes);
        for marker in [b"(kept-apart-one)".as_slice(), b"(kept-apart-two)"] {
            assert!(
                holds(&bytes, marker),
                "{name} did not keep {:?}",
                String::from_utf8_lossy(marker)
            );
        }
    }
    // One number stated in two runs that round to one double: the container states it twice, so a
    // produced file names one body there, and both sides hold one of the two whichever it is.
    let input = fixture(
        root.path(),
        "one-number-two-runs.pdf",
        document_engine::test_support::build::pdf_container_naming_one_number_in_two_digit_runs(),
    );
    let request = json!({
        "op": "pdf_rotate",
        "input": input.to_string_lossy(),
        "degrees": 90,
        "pages": "all",
    });
    let ours = produced(&engine, root.path(), "crate-container", &request);
    let theirs = produced(&reference, root.path(), "reference-container", &request);
    for (name, bytes) in [("the crate", &ours), ("the reference", &theirs)] {
        let bytes = searchable(bytes);
        let held = [b"(in-container-one)".as_slice(), b"(in-container-two)"]
            .iter()
            .filter(|marker| holds(&bytes, marker))
            .count();
        assert_eq!(held, 1, "{name}: one body of the two the one number states");
    }
}

/// A reference the file leaves dangling at the id the walk's first marker stands at, with the object
/// past the model that took that marker read before it: both sides write the reference back at the
/// number the file used — the crate from the id it restored, the reference because it never moved it —
/// so neither produced file re-points it at the object the marker named.
#[test]
fn a_dangling_reference_at_a_marker_number_is_kept_by_both_sides() {
    let (root, reference) = pair();
    let engine = CrateWriter;
    let input = fixture(
        root.path(),
        "marker-number.pdf",
        document_engine::test_support::build::pdf_dangling_reference_at_a_marker_number(),
    );
    let request = json!({
        "op": "pdf_rotate",
        "input": input.to_string_lossy(),
        "degrees": 90,
        "pages": "all",
    });
    let (said, ours) = carried_out(&engine, root.path(), "crate", &request);
    let theirs = produced(&reference, root.path(), "reference", &request);
    assert!(said.losses.is_empty(), "the crate lost nothing: {said:?}");
    for (name, bytes) in [("the crate", &ours), ("the reference", &theirs)] {
        assert!(
            holds(&searchable(bytes), b"4294967295 65535 R"),
            "{name} did not keep the reference where the file stated it"
        );
    }
    assert!(
        !ours
            .windows(b"4294967295 65535 obj".len())
            .any(|window| window == b"4294967295 65535 obj"),
        "the crate put an object at the number the reference names"
    );
}

/// A body the walk carried in place of a number: the second statement of one number does not parse,
/// so its bytes are what a produced file holds there, and the body the first statement wrote is in
/// neither file.
#[test]
fn a_carried_body_replacing_a_number_is_written_by_both_sides() {
    let (root, reference) = pair();
    let engine = CrateWriter;
    let input = fixture(
        root.path(),
        "carried-replacement.pdf",
        document_engine::test_support::build::pdf_carried_body_replacing_a_number(),
    );
    let request = json!({
        "op": "pdf_rotate",
        "input": input.to_string_lossy(),
        "degrees": 90,
        "pages": "all",
    });
    let (said, ours) = carried_out(&engine, root.path(), "crate", &request);
    let (theirs_said, theirs) = carried_out(&reference, root.path(), "reference", &request);
    assert_eq!(
        said.losses,
        vec![sentence("pdf_page_objects_lost", "objects", 1)],
        "the crate reports the body its carried one replaced"
    );
    assert!(
        theirs_said.losses.is_empty(),
        "the reference reports nothing: {theirs_said:?}"
    );
    for (name, bytes) in [("the crate", &ours), ("the reference", &theirs)] {
        assert!(
            holds(bytes, b"<< broken"),
            "{name} did not write the carried body"
        );
        assert!(
            !holds(bytes, b"NEEDLE"),
            "{name} wrote the body the carried one replaced"
        );
    }
}

/// A container whose `/N` is not a number: the reference's parser cannot build one, so both sides
/// carry the container as the bytes it states and place nothing the entries name — and the crate
/// reports no loss, because the file states no count.
#[test]
fn a_container_whose_count_is_not_a_number_is_carried_by_both_sides() {
    let (root, reference) = pair();
    let engine = CrateWriter;
    let input = fixture(
        root.path(),
        "count-name.pdf",
        document_engine::test_support::build::pdf_container_without_a_count(),
    );
    let request = json!({
        "op": "pdf_rotate",
        "input": input.to_string_lossy(),
        "degrees": 90,
        "pages": "all",
    });
    let (said, ours) = carried_out(&engine, root.path(), "crate", &request);
    let (theirs_said, theirs) = carried_out(&reference, root.path(), "reference", &request);
    assert!(said.losses.is_empty(), "the crate lost nothing: {said:?}");
    assert!(
        theirs_said.losses.is_empty(),
        "the reference reports nothing: {theirs_said:?}"
    );
    assert!(
        holds(&ours, b"/Type /ObjStm /N /Name"),
        "the crate did not carry the container as the bytes it states"
    );
    assert!(
        holds(&ours, b"/Type/Font"),
        "the crate wrote over the object the container names"
    );
    for (name, bytes) in [("the crate", &ours), ("the reference", &theirs)] {
        assert!(
            holds(&searchable(bytes), b"Page one text"),
            "{name} lost the page"
        );
    }
}

/// A field's `/Parent` and a form's `/Fields` entry stated as references that name a reference:
/// ascents and entries are read with the same one-step lookup, so both sides fault before they fill
/// anything rather than filling a form whose tree they cannot walk.
#[test]
fn a_chained_field_entry_faults_on_both_sides() {
    let (root, reference) = pair();
    for (name, bytes) in [
        (
            "field-parent",
            document_engine::test_support::build::pdf_field_parent_by_reference(),
        ),
        (
            "fields-entry",
            document_engine::test_support::build::pdf_fields_by_reference(),
        ),
    ] {
        let input = fixture(root.path(), &format!("{name}.pdf"), bytes);
        let request = json!({
            "op": "pdf_form_fill",
            "input": input.to_string_lossy(),
            "values": { "name": "x" },
        });
        for side in both_sides(&reference, root.path(), name, &request) {
            let who = side.who;
            let failure = side.declined();
            assert_eq!(
                failure.class, "internal",
                "{who} {name}: the engine's own failure: {failure:?}"
            );
        }
    }
}

/// The two arrays the field tree is built from, each stated as a reference to an array of its own: both
/// reads take one step, so the field the list names is a field of the reference's and is filled under
/// the name its `/Parent` chain builds. Refusing it would be a file the reference edits and this engine
/// does not.
#[test]
fn a_form_whose_list_is_stated_through_a_reference_is_filled_by_both_sides() {
    let (root, reference) = pair();
    for (name, bytes, values) in [
        (
            "fields-through-a-reference",
            document_engine::test_support::build::pdf_fields_through_a_reference(),
            json!({ "name": "x" }),
        ),
        (
            "kids-through-a-reference",
            document_engine::test_support::build::pdf_kids_through_a_reference(),
            json!({ "P.Q": "x" }),
        ),
    ] {
        let input = fixture(root.path(), &format!("{name}.pdf"), bytes);
        let request = json!({
            "op": "pdf_form_fill",
            "input": input.to_string_lossy(),
            "values": values,
        });
        for side in both_sides(&reference, root.path(), name, &request) {
            let who = side.who;
            let written = side.carried_out();
            assert!(
                holds(&searchable(&written), b"/V"),
                "{who} {name}: the field the list names was not filled"
            );
        }
    }
}

/// A kid stated where it stands is no field of a container's: `createPDFAcroFields` keeps a kid only
/// when the array states a reference that names a dictionary. Both sides therefore have no field to
/// fill and refuse the value as one the form does not hold — the engine must not register the kid and
/// write a value into a file the reference never sees it in.
#[test]
fn a_kid_stated_in_place_is_no_field_on_either_side() {
    let (root, reference) = pair();
    let input = fixture(
        root.path(),
        "kid-in-place.pdf",
        document_engine::test_support::build::pdf_field_kid_in_place(),
    );
    let request = json!({
        "op": "pdf_form_fill",
        "input": input.to_string_lossy(),
        "values": { "P.Q": "x" },
    });
    let mut said = Vec::new();
    for side in both_sides(&reference, root.path(), "kid-in-place", &request) {
        let who = side.who;
        let failure = side.declined();
        assert_eq!(
            failure.class, "usage",
            "{who}: a form with no field is a refusal: {failure:?}"
        );
        said.push(failure.reason.clone());
    }
    assert_eq!(said[0], said[1], "both sides name the same fields");
}

/// A field whose own entry is stated as a reference that names nothing, and a field whose `/FT` is
/// null under a parent that states one: `lookupMaybe` answers undefined for a reference that names
/// nothing and `getInheritableAttribute` skips a stated null, so neither is a fault and both sides fill
/// the field they name.
#[test]
fn a_field_read_past_a_null_entry_is_filled_by_both_sides() {
    let (root, reference) = pair();
    for (name, bytes, values) in [
        (
            "name-names-nothing",
            document_engine::test_support::build::pdf_field_name_that_names_nothing(),
            json!({ "P.undefined": "x" }),
        ),
        (
            "type-past-a-null",
            document_engine::test_support::build::pdf_field_type_inherited_past_a_null(),
            json!({ "P.Q": "x" }),
        ),
    ] {
        let input = fixture(root.path(), &format!("{name}.pdf"), bytes);
        let request = json!({
            "op": "pdf_form_fill",
            "input": input.to_string_lossy(),
            "values": values,
        });
        for side in both_sides(&reference, root.path(), name, &request) {
            let who = side.who;
            let written = side.carried_out();
            assert!(
                holds(&searchable(&written), b"/V"),
                "{who} {name}: the field was not filled"
            );
        }
    }
}

/// A text field the request does not fill states a normal appearance and a `/Ff` that is a reference
/// to a reference: the reference reads a text field's flags only where it regenerates that field's
/// appearance, so the flags are never read and the fill produces — which it must here too.
#[test]
fn a_clean_field_whose_flags_are_a_reference_is_no_fault_on_either_side() {
    let (root, reference) = pair();
    let input = fixture(
        root.path(),
        "clean-field.pdf",
        document_engine::test_support::build::pdf_clean_field_whose_flags_are_a_reference(),
    );
    let request = json!({
        "op": "pdf_form_fill",
        "input": input.to_string_lossy(),
        "values": { "f0": "x" },
    });
    for side in both_sides(&reference, root.path(), "clean-field", &request) {
        let who = side.who;
        let written = side.carried_out();
        assert!(
            holds(&searchable(&written), b"/V"),
            "{who}: the field the request names was not filled"
        );
    }
}

/// A field that states an empty `/Kids` array draws no widget: both sides write the value and no
/// appearance for it, where a field that states no `/Kids` at all draws itself.
#[test]
fn a_field_with_no_widgets_writes_no_appearance_on_either_side() {
    let (root, reference) = pair();
    let input = fixture(
        root.path(),
        "no-widgets.pdf",
        document_engine::test_support::build::pdf_field_with_no_widgets(),
    );
    let request = json!({
        "op": "pdf_form_fill",
        "input": input.to_string_lossy(),
        "values": { "f0": "x" },
    });
    for side in both_sides(&reference, root.path(), "no-widgets", &request) {
        let who = side.who;
        let written = side.carried_out();
        let searchable = searchable(&written);
        assert!(
            holds(&searchable, b"/V"),
            "{who}: the value was not written"
        );
        assert!(
            !holds(&searchable, b"/AP"),
            "{who}: an appearance was drawn for a field with no widget"
        );
    }
}

/// A kid that is not a dictionary: the typed widget read throws on it for every fillable kind, so both
/// sides fault rather than filling a form whose widgets cannot be read.
#[test]
fn a_kid_that_is_not_a_dictionary_faults_on_both_sides() {
    let (root, reference) = pair();
    let input = fixture(
        root.path(),
        "kid-is-a-number.pdf",
        document_engine::test_support::build::pdf_field_kid_that_is_not_a_dictionary(),
    );
    let request = json!({
        "op": "pdf_form_fill",
        "input": input.to_string_lossy(),
        "values": { "f0": "x" },
    });
    for side in both_sides(&reference, root.path(), "kid-is-a-number", &request) {
        let who = side.who;
        let failure = side.declined();
        assert_eq!(
            failure.class, "internal",
            "{who}: the widget read is the engine's own failure: {failure:?}"
        );
    }
}

/// A `/Ff` stated as a reference that names nothing: `Ff()` is a `lookupMaybe`, which answers no flags
/// for a value that names nothing, so the field is a list box of its own options on both sides. A typed
/// read here would fault a form the reference fills.
#[test]
fn a_flags_entry_that_names_nothing_is_no_fault_on_either_side() {
    let (root, reference) = pair();
    let input = fixture(
        root.path(),
        "flags-name-nothing.pdf",
        document_engine::test_support::build::pdf_choice_field_whose_flags_name_nothing(),
    );
    let request = json!({
        "op": "pdf_form_fill",
        "input": input.to_string_lossy(),
        "values": { "f0": "a" },
    });
    for side in both_sides(&reference, root.path(), "flags-name-nothing", &request) {
        let who = side.who;
        let written = side.carried_out();
        assert!(
            holds(&searchable(&written), b"/V"),
            "{who}: the field the request names was not filled"
        );
    }
}

/// A `/Parent` stated as a dictionary in place: the constructor's `ascend` reads each link with
/// `lookupMaybe(…, PDFDict)`, which follows one, so the field inherits the parent's type; its name is
/// still its own, because `getParent` follows a reference only. Both sides fill it under `Q`.
#[test]
fn a_parent_stated_in_place_is_filled_by_both_sides() {
    let (root, reference) = pair();
    let input = fixture(
        root.path(),
        "parent-in-place.pdf",
        document_engine::test_support::build::pdf_field_parent_in_place(),
    );
    let request = json!({
        "op": "pdf_form_fill",
        "input": input.to_string_lossy(),
        "values": { "Q": "x" },
    });
    for side in both_sides(&reference, root.path(), "parent-in-place", &request) {
        let who = side.who;
        let written = side.carried_out();
        assert!(
            holds(&searchable(&written), b"/V"),
            "{who}: the field the parent types was not filled"
        );
    }
}

/// A `/Kids` that is a number rather than an array: `Kids()` is a typed `lookupMaybe`, which throws on
/// it where the appearance is regenerated, so both sides fault rather than producing a file in which
/// the field drew itself.
#[test]
fn a_kids_entry_that_is_not_an_array_faults_on_both_sides() {
    let (root, reference) = pair();
    let input = fixture(
        root.path(),
        "kids-is-a-number.pdf",
        document_engine::test_support::build::pdf_field_kids_that_is_not_an_array(),
    );
    let request = json!({
        "op": "pdf_form_fill",
        "input": input.to_string_lossy(),
        "values": { "f0": "x" },
    });
    for side in both_sides(&reference, root.path(), "kids-is-a-number", &request) {
        let who = side.who;
        let failure = side.declined();
        assert_eq!(
            failure.class, "internal",
            "{who}: the widget read is the engine's own failure: {failure:?}"
        );
    }
}

/// A malformed widget kid beside a request that names no field: the reference applies the values before
/// it reads any widget list, so the answer is the usage refusal that lists the form's fields — not the
/// fault the same form gives when a field it holds is filled.
#[test]
fn a_bad_widget_kid_does_not_turn_a_missing_field_into_a_fault() {
    let (root, reference) = pair();
    let input = fixture(
        root.path(),
        "bad-kid-missing-field.pdf",
        document_engine::test_support::build::pdf_field_kid_that_is_not_a_dictionary(),
    );
    let request = json!({
        "op": "pdf_form_fill",
        "input": input.to_string_lossy(),
        "values": { "zzz": "x" },
    });
    let mut said = Vec::new();
    for side in both_sides(&reference, root.path(), "bad-kid-missing-field", &request) {
        let who = side.who;
        let failure = side.declined();
        assert_eq!(
            failure.class, "usage",
            "{who}: a name the form does not hold is the caller's problem: {failure:?}"
        );
        said.push((who, failure.reason.clone()));
    }
    assert_eq!(
        said[0].1, said[1].1,
        "both sides name the same fields: {said:?}"
    );
}

/// A choice field whose `/Ff` is the negative real `-1.5`: the reference tests the flags with
/// JavaScript's `&`, which converts `-1.5` to the 32-bit integer `-1`, so every bit is set and the
/// field is a combo box that accepts a value outside its own list. Both sides produce, and both write
/// the flags back as the integer those bits are.
#[test]
fn a_negative_real_flags_is_a_combo_box_on_both_sides() {
    let (root, reference) = pair();
    let input = fixture(
        root.path(),
        "negative-real-flags.pdf",
        document_engine::test_support::build::pdf_choice_field_whose_flags_are_a_negative_real(),
    );
    let request = json!({
        "op": "pdf_form_fill",
        "input": input.to_string_lossy(),
        "values": { "f0": "x" },
    });
    for side in both_sides(&reference, root.path(), "negative-real-flags", &request) {
        let who = side.who;
        let written = side.carried_out();
        assert!(
            holds(&searchable(&written), b"/Ff -1"),
            "{who}: the flags are not the 32-bit integer the reference reads"
        );
    }
}

/// The same negative real on a button field: the 32-bit read sets the push-button bit, which the
/// reference tests before the radio bit, so the field is one no fill can take a value for.
#[test]
fn a_negative_real_flags_on_a_button_is_refused_by_both_sides() {
    let (root, reference) = pair();
    let input = fixture(
        root.path(),
        "negative-real-button.pdf",
        document_engine::test_support::build::pdf_button_field_whose_flags_are_a_negative_real(),
    );
    let request = json!({
        "op": "pdf_form_fill",
        "input": input.to_string_lossy(),
        "values": { "f0": "x" },
    });
    let mut said = Vec::new();
    for side in both_sides(&reference, root.path(), "negative-real-button", &request) {
        let who = side.who;
        let failure = side.declined();
        assert_eq!(
            failure.class, "usage",
            "{who}: a push button is not a fillable field: {failure:?}"
        );
        said.push((who, failure.reason.clone()));
    }
    assert_eq!(
        said[0].1, said[1].1,
        "both sides print the reference's own sentence: {said:?}"
    );
}

/// A choice field whose `/Ff` is a real past 2^32: the 32-bit read keeps no bit the kind reads, so
/// the field is a list box and the value is not one of its own. Both sides refuse with the same
/// sentence.
#[test]
fn a_huge_real_flags_is_a_list_box_on_both_sides() {
    let (root, reference) = pair();
    let input = fixture(
        root.path(),
        "huge-real-flags.pdf",
        document_engine::test_support::build::pdf_choice_field_whose_flags_are_a_huge_real(),
    );
    let request = json!({
        "op": "pdf_form_fill",
        "input": input.to_string_lossy(),
        "values": { "f0": "x" },
    });
    let mut said = Vec::new();
    for side in both_sides(&reference, root.path(), "huge-real-flags", &request) {
        let who = side.who;
        let failure = side.declined();
        assert_eq!(
            failure.class, "usage",
            "{who}: a list box takes one of its options: {failure:?}"
        );
        said.push((who, failure.reason.clone()));
    }
    assert_eq!(
        said[0].1, said[1].1,
        "both sides name the same options: {said:?}"
    );
}

/// A `/Ff` real the model's 32-bit float cannot state exactly: `4294967295.5` is `-1` to the
/// reference's double and 2^32 to this model's float, so the reference makes the field a combo box
/// and produces while the crate makes it a list box and refuses. The band is stated in `PARSER.md`
/// §5.16; this is its measured witness.
#[test]
fn a_flags_real_past_the_float_is_read_differently_by_the_two_sides() {
    let (root, reference) = pair();
    let input = fixture(
        root.path(),
        "flags-past-the-float.pdf",
        document_engine::test_support::build::pdf_choice_field_whose_flags_are_a_real_past_the_float(),
    );
    let request = json!({
        "op": "pdf_form_fill",
        "input": input.to_string_lossy(),
        "values": { "f0": "x" },
    });
    let [crate_side, reference_side] =
        both_sides(&reference, root.path(), "flags-past-the-float", &request);
    reference_side.carried_out();
    let failure = crate_side.declined();
    assert_eq!(
        failure.class, "usage",
        "the crate reads the rounded float as a list box: {failure:?}"
    );
}

/// A `/MaxLen` stated as the real `3.5`: the reference reads it as a number and faults a six-character
/// value, printing the real in its sentence, where the engine reads only a whole number and writes the
/// value. The band is stated in `PARSER.md` §5.16; this is its measured witness.
#[test]
fn a_real_max_length_bounds_the_reference_and_not_the_crate() {
    let (root, reference) = pair();
    let input = fixture(
        root.path(),
        "real-max-length.pdf",
        document_engine::test_support::build::pdf_real_max_length_form(),
    );
    let request = json!({
        "op": "pdf_form_fill",
        "input": input.to_string_lossy(),
        "values": { "name": "abcdef" },
    });
    let [crate_side, reference_side] =
        both_sides(&reference, root.path(), "real-max-length", &request);
    let failure = reference_side.declined();
    assert_eq!(
        failure.class, "internal",
        "the reference enforces the real bound: {failure:?}"
    );
    assert!(
        failure.reason.contains("maxLength=3.5"),
        "the reference prints the real it read: {}",
        failure.reason
    );
    crate_side.carried_out();
}

/// A field whose kid list returns to its own container: the reference's `getAllFields` recurses on a
/// non-terminal field and dies with a stack overflow, where the walk ends at the cycle and answers
/// with the usage refusal that names the empty catalogue.
#[test]
fn a_field_kids_cycle_faults_only_on_the_reference() {
    let (root, reference) = pair();
    let input = fixture(
        root.path(),
        "kids-cycle.pdf",
        document_engine::test_support::build::pdf_field_kids_cycle(),
    );
    let request = json!({
        "op": "pdf_form_fill",
        "input": input.to_string_lossy(),
        "values": { "P.Q": "x" },
    });
    let [crate_side, reference_side] = both_sides(&reference, root.path(), "kids-cycle", &request);
    assert_eq!(
        crate_side.declined().class,
        "usage",
        "the cycle ends here and yields no field"
    );
    assert_eq!(
        reference_side.declined().class,
        "internal",
        "the reference dies on the cycle"
    );
}

/// A field whose `/Parent` returns to itself: the reference's name read and its constructor's
/// inherited-entry walk both recurse on the link and die with a stack overflow, where the walk stops
/// at the repeated link and fills the field by the one name it states.
#[test]
fn a_field_parent_cycle_faults_only_on_the_reference() {
    let (root, reference) = pair();
    let input = fixture(
        root.path(),
        "parent-cycle.pdf",
        document_engine::test_support::build::pdf_field_parent_cycle(),
    );
    let request = json!({
        "op": "pdf_form_fill",
        "input": input.to_string_lossy(),
        "values": { "P": "x" },
    });
    let [crate_side, reference_side] =
        both_sides(&reference, root.path(), "parent-cycle", &request);
    let failure = reference_side.declined();
    assert_eq!(
        failure.class, "internal",
        "the reference dies on the link: {failure:?}"
    );
    crate_side.carried_out();
}

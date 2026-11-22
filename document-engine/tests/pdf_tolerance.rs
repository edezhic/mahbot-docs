//! The PDF input tolerance the engine claims, shape by shape.
//!
//! Each test names a file the reference reads and edits — or one both sides refuse — and pins the
//! answer: no refusal for a file the reference produces, and a loss sentence for anything the
//! produced file does not hold or no longer names. `PARSER.md` is the enumeration these witness.

use std::path::Path;
use std::time::Instant;

use document_engine::operation::{Destination, Outcome, Request, execute};
use document_engine::test_support::build;
use document_engine::vocabulary::vocabulary;
use serde_json::{Value, json};

/// Run one operation over one fixture and answer what the engine said.
fn run(
    directory: &Path,
    operation: &str,
    fixture: &[u8],
    base: &str,
    mut values: Value,
) -> Outcome {
    let input = directory.join(format!("{base}-in.pdf"));
    std::fs::write(&input, fixture).expect("the fixture is written");
    values["input"] = json!(input.to_string_lossy());
    execute(&Request {
        operation: operation.to_string(),
        values: values.as_object().cloned().unwrap_or_default(),
        destination: Destination {
            directory: directory.to_path_buf(),
            base_name: base.to_string(),
        },
    })
}

/// Rotate every page, the operation that rewrites every object of the file.
fn rotate(directory: &Path, fixture: &[u8], base: &str) -> Outcome {
    run(
        directory,
        "pdf_rotate",
        fixture,
        base,
        json!({ "degrees": 90, "pages": "all" }),
    )
}

/// The one file a call produced, and its bytes.
fn produced(outcome: &Outcome) -> Vec<u8> {
    assert!(outcome.failure.is_none(), "the call failed: {outcome:?}");
    let path = outcome.outputs.first().expect("a produced file");
    std::fs::read(path).expect("the produced file")
}

/// The sentence a loss of `mark` states over `count`.
fn sentence(mark: &str, field: &str, count: usize) -> String {
    vocabulary()
        .marks
        .format(mark, &[(field, &count.to_string())])
}

#[test]
fn a_container_whose_entries_state_one_offset_is_read() {
    // Every entry of the object stream names the same bytes: the reference parses the entry per
    // offset the file states and produces, so refusing the file would refuse one it edits.
    let directory = tempfile::tempdir().expect("a temporary directory");
    let outcome = rotate(
        directory.path(),
        &build::pdf_object_stream_one_offset(),
        "one-offset",
    );
    let bytes = produced(&outcome);
    assert!(outcome.losses.is_empty(), "nothing is lost: {outcome:?}");
    assert!(
        String::from_utf8_lossy(&bytes).contains("/Rotate 90"),
        "the page was not turned"
    );
}

#[test]
fn a_container_stating_a_large_early_change_is_produced() {
    // `/EarlyChange` is the number the file states, and the reference puts it into its arithmetic: a
    // single-precision read rounds 33554173 to 33554172, which moves the code width a code later and
    // leaves the container unreadable. The engine reads the number exactly, so its objects are placed.
    let directory = tempfile::tempdir().expect("a temporary directory");
    let outcome = rotate(
        directory.path(),
        &build::pdf_object_stream_lzw_early_change(),
        "lzw-early-change",
    );
    let bytes = produced(&outcome);
    assert!(outcome.losses.is_empty(), "nothing is lost: {outcome:?}");
    assert!(
        String::from_utf8_lossy(&bytes).contains("/Rotate 90"),
        "the page was not turned"
    );
}

#[test]
fn a_container_whose_lzw_stream_is_cut_short_is_expanded() {
    // The reference's decoder pads the block a stream ends inside with zero codes, so a container it
    // decodes only partly is expanded rather than carried: the objects before the cut are found and
    // the file is edited.
    let directory = tempfile::tempdir().expect("a temporary directory");
    let outcome = rotate(
        directory.path(),
        &build::pdf_object_stream_truncated_lzw(),
        "lzw-cut",
    );
    let bytes = produced(&outcome);
    assert!(outcome.losses.is_empty(), "nothing is lost: {outcome:?}");
    assert!(
        String::from_utf8_lossy(&bytes).contains("/Rotate 90"),
        "the page was not turned"
    );
}

#[test]
fn an_empty_lzw_index_stream_adopts_its_trailer_where_a_carried_one_does_not() {
    // An empty `LZWDecode` body decodes to the zero block the reference pads a stream with, so the
    // index stream is decoded rather than carried and its `/Encrypt` is adopted: the file needs a
    // password. The same stream under a filter neither side reads is carried, its trailer is never
    // adopted, and the file is the unencrypted one its body states. That is the difference `Some` and
    // `None` decide, and neither is a refusal of the file for being unreadable.
    let directory = tempfile::tempdir().expect("a temporary directory");
    let refused = rotate(
        directory.path(),
        &build::pdf_object_stream_empty_lzw_index(),
        "empty-index",
    );
    assert_eq!(
        refused
            .failure
            .as_ref()
            .map(|failure| failure.reason.as_str()),
        Some(
            vocabulary()
                .marks
                .format("pdf_page_encrypted", &[("name", "empty-index-in.pdf")])
                .as_str()
        )
    );
    let outcome = rotate(
        directory.path(),
        &build::pdf_object_stream_undecodable_index(),
        "unknown-index",
    );
    let bytes = produced(&outcome);
    assert!(
        String::from_utf8_lossy(&bytes).contains("/Rotate 90"),
        "the page was not turned"
    );
}

#[test]
fn a_container_stating_a_count_no_container_could_hold_is_turned() {
    // `/N` is the file's own statement and a file may state one nothing could hold. The reference keeps
    // the container as the bytes it states and produces; the count the answer reports is that statement,
    // attributed to the file, because nothing the walk read says how many objects the container held.
    let directory = tempfile::tempdir().expect("a temporary directory");
    let outcome = rotate(
        directory.path(),
        &build::pdf_container_stating_an_absurd_count(),
        "absurd-count",
    );
    produced(&outcome);
    assert_eq!(
        outcome.losses,
        vec![sentence(
            "pdf_page_objects_unread",
            "objects",
            9_223_372_036_854_775_807
        )],
        "the number is the one the file states, not one the engine counted: {outcome:?}"
    );
}

#[test]
fn a_container_whose_count_is_not_a_number_is_carried() {
    // The reference's constructor reads `/N` with a typed lookup, so a container that states a name
    // there is one it cannot build: it is carried as the bytes it states and nothing the entries name
    // is placed. Nothing is reported either — the file states no count — and, above all, no object the
    // container happens to number is written over: object 1 is still the font.
    let directory = tempfile::tempdir().expect("a temporary directory");
    let outcome = rotate(
        directory.path(),
        &build::pdf_container_without_a_count(),
        "count-name",
    );
    let bytes = produced(&outcome);
    assert!(outcome.losses.is_empty(), "nothing is lost: {outcome:?}");
    assert!(
        bytes.windows(14).any(|window| window == b"/Type /ObjStm "),
        "the container is not carried as the bytes it states"
    );
    assert!(
        bytes.windows(10).any(|window| window == b"/Type/Font"),
        "the object the container names was written over"
    );
}

#[test]
fn a_reference_inside_a_carried_body_is_reported_when_its_object_moves() {
    // The object at `5000000000 0 obj` is given a number of this document's own, and the bytes of
    // the body the walk carried are written back verbatim: the reference inside them named that
    // object and now names nothing. The answer says so.
    let directory = tempfile::tempdir().expect("a temporary directory");
    let outcome = rotate(
        directory.path(),
        &build::pdf_carried_reference_to_a_placed_object(),
        "carried",
    );
    let bytes = produced(&outcome);
    assert!(
        bytes.windows(14).any(|window| window == b"5000000000 0 R"),
        "the carried body is written back as the file stated it"
    );
    assert_eq!(
        outcome.losses,
        vec![sentence("pdf_page_references_lost", "references", 1)],
        "the reference that lost its object is reported"
    );
    assert_eq!(outcome.notes, outcome.losses, "a loss is a note as well");
}

#[test]
fn a_placed_object_does_not_take_a_number_a_carried_reference_names() {
    // Object 201 is the number the walk would give the first object it places, and a body the walk
    // carried names it. A number given to a placed object is one no reference the file states names,
    // so that reference keeps naming nothing.
    let directory = tempfile::tempdir().expect("a temporary directory");
    let outcome = rotate(
        directory.path(),
        &build::pdf_carried_reference_to_a_free_number(),
        "free",
    );
    let bytes = produced(&outcome);
    assert!(outcome.losses.is_empty(), "nothing is lost: {outcome:?}");
    assert!(
        !bytes.windows(11).any(|window| window == b"\n201 0 obj\n"),
        "the placed object took the number the carried reference names"
    );
}

#[test]
fn a_saturated_container_header_loses_nothing() {
    // The container a header past a `u64` states is expanded and discarded, so a second such header
    // replaces nothing a produced file would hold.
    let directory = tempfile::tempdir().expect("a temporary directory");
    let outcome = rotate(
        directory.path(),
        &build::pdf_saturated_container_header(),
        "saturated-container",
    );
    produced(&outcome);
    assert!(
        outcome.losses.is_empty(),
        "nothing was replaced: {outcome:?}"
    );
}

#[test]
fn a_container_after_a_saturated_object_loses_nothing() {
    // A container is expanded and discarded rather than kept, so the object the earlier saturated
    // header states is still in the produced file: the header after it replaced nothing.
    let directory = tempfile::tempdir().expect("a temporary directory");
    let outcome = rotate(
        directory.path(),
        &build::pdf_container_after_a_saturated_object(),
        "container-after",
    );
    produced(&outcome);
    assert!(
        outcome.losses.is_empty(),
        "nothing was replaced: {outcome:?}"
    );
}

#[test]
fn two_numbers_longer_than_a_u64_are_reported_as_the_object_they_lose() {
    // Both runs of digits round to one double, which is the number the reference keys an object by:
    // the second header states the object the first one stated, so the produced file holds one body
    // there and the answer names the one it no longer holds.
    let directory = tempfile::tempdir().expect("a temporary directory");
    let outcome = rotate(
        directory.path(),
        &build::pdf_two_numbers_longer_than_a_u64(),
        "saturated-objects",
    );
    produced(&outcome);
    assert_eq!(
        outcome.losses,
        vec![sentence("pdf_page_objects_lost", "objects", 1)],
        "the object the second header replaced is reported"
    );
}

#[test]
fn two_numbers_past_the_model_that_the_reference_keys_apart_are_both_kept() {
    // The two runs round to two doubles, so they are two numbers to the reference and both bodies
    // reach the produced file. `pdf_two_numbers_longer_than_a_u64` is the other side of the same
    // rule: there the runs are one double, and one object is lost.
    let directory = tempfile::tempdir().expect("a temporary directory");
    let outcome = rotate(
        directory.path(),
        &build::pdf_two_numbers_the_reference_keys_apart(),
        "kept-apart",
    );
    let bytes = produced(&outcome);
    assert!(outcome.losses.is_empty(), "nothing is lost: {outcome:?}");
    for marker in [
        b"(kept-apart-one)".as_slice(),
        b"(kept-apart-two)".as_slice(),
    ] {
        assert!(
            bytes.windows(marker.len()).any(|window| window == marker),
            "{:?} is not in the produced bytes",
            String::from_utf8_lossy(marker)
        );
    }
}

#[test]
fn a_container_naming_two_numbers_the_reference_keys_apart_keeps_both() {
    // A container's entries are keyed as an object's header is, by the double their digits state:
    // these two runs are two doubles, so both bodies the container holds reach the produced file.
    let directory = tempfile::tempdir().expect("a temporary directory");
    let outcome = rotate(
        directory.path(),
        &build::pdf_container_naming_two_numbers_the_reference_keys_apart(),
        "container-apart",
    );
    let bytes = produced(&outcome);
    assert!(outcome.losses.is_empty(), "nothing is lost: {outcome:?}");
    for marker in [
        b"(in-container-one)".as_slice(),
        b"(in-container-two)".as_slice(),
    ] {
        assert!(
            bytes.windows(marker.len()).any(|window| window == marker),
            "{:?} is not in the produced bytes",
            String::from_utf8_lossy(marker)
        );
    }
}

#[test]
fn a_container_naming_one_number_in_two_digit_runs_loses_one() {
    // Both runs round to one double, so the container names one object twice: a produced file names
    // one body per number, and the answer says which body it does not hold.
    let directory = tempfile::tempdir().expect("a temporary directory");
    let outcome = rotate(
        directory.path(),
        &build::pdf_container_naming_one_number_in_two_digit_runs(),
        "container-one",
    );
    produced(&outcome);
    assert_eq!(
        outcome.losses,
        vec![sentence("pdf_page_objects_lost", "objects", 1)],
        "the entry the second one replaced is reported"
    );
}

#[test]
fn a_carried_body_that_replaces_a_number_is_reported() {
    // Both headers state one number — the runs round to one double — and the second body does not
    // parse, so the bytes the walk carried are what the produced file holds there. The body the
    // first header stated is not in that file, and the answer says so rather than letting the carried
    // bytes take its place silently.
    let directory = tempfile::tempdir().expect("a temporary directory");
    let outcome = rotate(
        directory.path(),
        &build::pdf_carried_body_replacing_a_number(),
        "carried-replacement",
    );
    let bytes = produced(&outcome);
    assert_eq!(
        outcome.losses,
        vec![sentence("pdf_page_objects_lost", "objects", 1)],
        "the object the carried body replaced is reported"
    );
    let carried = b"<< broken";
    assert!(
        bytes.windows(carried.len()).any(|window| window == carried),
        "the carried body is not written back"
    );
    assert!(
        !bytes.windows(6).any(|window| window == b"NEEDLE"),
        "the body the carried one replaced is still in the produced file"
    );
}

#[test]
fn two_generations_of_one_number_are_reported_as_the_body_the_index_cannot_name() {
    let directory = tempfile::tempdir().expect("a temporary directory");
    let outcome = rotate(
        directory.path(),
        &build::pdf_two_generations_of_one_number(),
        "generations",
    );
    let bytes = produced(&outcome);
    assert_eq!(
        outcome.losses,
        vec![sentence("pdf_page_objects_lost", "objects", 1)],
        "the index names one body per number, so one of the two is reported"
    );
    assert!(
        bytes.windows(10).any(|window| window == b"\n20 0 obj\n")
            || bytes.windows(10).any(|window| window == b"\n20 1 obj\n"),
        "a body is written for the number"
    );
}

#[test]
fn an_object_at_number_zero_is_left_out_and_reported() {
    // The index states no entry at number zero, so an object there is one a produced file cannot
    // hold. The reference keeps it and shifts its index; the answer here says what it lost.
    let directory = tempfile::tempdir().expect("a temporary directory");
    let outcome = rotate(
        directory.path(),
        &build::pdf_object_at_number_zero(),
        "zero",
    );
    let bytes = produced(&outcome);
    assert_eq!(
        outcome.losses,
        vec![sentence("pdf_page_objects_lost", "objects", 1)],
        "the object at number zero is reported"
    );
    assert!(
        !bytes.windows(9).any(|window| window == b"\n0 1 obj\n"),
        "no object is written at number zero"
    );
}

#[test]
fn a_container_stating_a_fractional_count_reads_both_its_entries() {
    // The reference counts `for (idx = 0; idx < 1.5; idx++)`, so a fractional count reads the entries
    // below the next whole number above it. Truncated to its whole part it would read one and drop the
    // other, which is a silent loss however damaged the file is.
    let directory = tempfile::tempdir().expect("a temporary directory");
    let outcome = rotate(
        directory.path(),
        &build::pdf_container_with_a_fractional_count(),
        "fractional",
    );
    let bytes = produced(&outcome);
    assert!(outcome.losses.is_empty(), "nothing is lost: {outcome:?}");
    for object in [b"\n910 0 obj\n".as_slice(), b"\n911 0 obj\n".as_slice()] {
        assert!(
            bytes.windows(object.len()).any(|window| window == object),
            "{:?} is not in the produced bytes",
            String::from_utf8_lossy(object)
        );
    }
}

#[test]
fn a_container_that_decodes_and_states_a_count_past_its_bytes_reports_it() {
    // The container decodes and its `/N` names far more entries than its bytes hold: the reference's
    // parser runs out of entries, keeps the container as the bytes it states and places none, so the
    // count the answer reports is the file's own statement.
    let directory = tempfile::tempdir().expect("a temporary directory");
    let outcome = rotate(
        directory.path(),
        &build::pdf_container_count_past_its_bytes(),
        "past-bytes",
    );
    let bytes = produced(&outcome);
    assert_eq!(
        outcome.losses,
        vec![sentence("pdf_page_objects_unread", "objects", 4096)],
        "the count is the container's own `/N`"
    );
    assert!(
        bytes.windows(13).any(|window| window == b"/Type /ObjStm"),
        "the container is carried as the bytes it states"
    );
}

#[test]
fn an_object_at_number_zero_at_the_marker_generation_is_renumbered() {
    // Number zero at the marker generation is a pair the model cannot key like the generation itself,
    // so the object is held the way one past the model is and given a number of its own: nothing is
    // lost, because nothing the produced file could name was there.
    let directory = tempfile::tempdir().expect("a temporary directory");
    let outcome = rotate(
        directory.path(),
        &build::pdf_object_at_number_zero_at_the_marker_generation(),
        "zero-marker",
    );
    let bytes = produced(&outcome);
    assert!(outcome.losses.is_empty(), "nothing is lost: {outcome:?}");
    assert!(
        bytes.windows(13).any(|window| window == b"(zero-marker)"),
        "the object is in the produced file"
    );
    assert!(
        !bytes.windows(12).any(|window| window == b"\n0 65535 obj"),
        "no object is written at the number the file stated"
    );
}

#[test]
fn an_empty_field_name_is_never_a_field_and_is_never_listed() {
    // The reference tests the name before it asks the form for it, so an empty name is the refusal that
    // names it `""` even where the file states a field whose name is empty — and its catalogue skips an
    // empty name, so the field is not listed either.
    let directory = tempfile::tempdir().expect("a temporary directory");
    let outcome = run(
        directory.path(),
        "pdf_form_fill",
        &build::pdf_form_with_a_nameless_field(),
        "nameless",
        json!({ "values": { "": "Ada" } }),
    );
    let failure = outcome.failure.expect("the empty name is refused");
    assert!(
        failure.class == "usage",
        "the caller can name a field instead: {failure:?}"
    );
    assert!(
        failure.reason.starts_with("no such form field: \"\""),
        "the refusal names the empty name: {}",
        failure.reason
    );
    assert!(
        failure
            .reason
            .contains(vocabulary().marks.get("pdf_form_catalogue_none")),
        "the form lists no field, because an empty name is not listed: {}",
        failure.reason
    );
}

#[test]
fn a_field_kids_cycle_ends_where_the_reference_dies() {
    // The reference's `getAllFields` recurses on a non-terminal field and dies with a stack overflow
    // on this shape (measured, probe row "a form whose field kid list returns to its own container");
    // the walk ends at the cycle, so the form holds no field and the request is a usage refusal.
    let directory = tempfile::tempdir().expect("a temporary directory");
    let outcome = run(
        directory.path(),
        "pdf_form_fill",
        &build::pdf_field_kids_cycle(),
        "kids-cycle",
        json!({ "values": { "P.Q": "x" } }),
    );
    let failure = outcome.failure.expect("a cycle yields no field to fill");
    assert_eq!(failure.class, "usage");
    assert!(
        failure.reason.starts_with("no such form field: P.Q"),
        "the cycle is a form with no field: {}",
        failure.reason
    );
}

#[test]
fn a_field_parent_cycle_ends_where_the_reference_dies() {
    // The reference's `getFullyQualifiedName` and its constructor's `ascend` both recurse on
    // `/Parent` and die with a stack overflow on this shape (measured, probe row "a form whose field
    // parent link returns to itself"); the name stops at the repeated link, so the field is named by
    // the one link it states and is filled.
    let directory = tempfile::tempdir().expect("a temporary directory");
    let outcome = run(
        directory.path(),
        "pdf_form_fill",
        &build::pdf_field_parent_cycle(),
        "parent-cycle",
        json!({ "values": { "P": "x" } }),
    );
    assert!(
        outcome.failure.is_none(),
        "the field is named by the one link it states: {outcome:?}"
    );
}

#[test]
fn a_field_tree_twenty_thousand_deep_is_filled_on_the_harness_stack() {
    // The walk hands each container's kids to a worklist instead of recursing. Measured, the reference
    // fills this shape in 0.80 s, and a recursive walk ends the process — it does not answer it — at a
    // thousand levels on the 2 MiB stack the harness gives the engine, so no depth bound can stand in
    // for the worklist: any bound low enough to fit the stack refuses a tree the reference fills.
    let directory = tempfile::tempdir().expect("a temporary directory");
    let fixture = deep_form(20_000);
    let root = directory.path().to_path_buf();
    let outcome = std::thread::Builder::new()
        .stack_size(2 * 1024 * 1024)
        .spawn(move || {
            run(
                &root,
                "pdf_form_fill",
                &fixture,
                "deep",
                json!({ "values": { "Q": "x" }, "pages": "all" }),
            )
        })
        .expect("a thread")
        .join()
        .expect("the walk answers rather than ending the process");
    assert!(
        outcome.failure.is_none(),
        "the deep form is filled: {outcome:?}"
    );
}

/// A form of `depth` nested containers over one text field named by its own `/T`.
fn deep_form(depth: usize) -> Vec<u8> {
    let mut pdf = build::PdfObjects::default();
    let font = pdf.push(build::PDF_FONT);
    let pages = pdf.reserve();
    let page = pdf.reserve();
    let content = pdf.stream("", b"");
    let acroform = pdf.reserve();
    let field = pdf.push(format!(
        "<< /Type /Annot /Subtype /Widget /Rect [72 600 272 700] /F 4 /P {page} \
         /DA (/Helv 0 Tf 0 g) /FT /Tx /T (Q) >>"
    ));
    let mut outermost = field.clone();
    for _ in 0..depth {
        outermost = pdf.push(format!("<< /T (P) /Kids [{outermost}] >>"));
    }
    pdf.set(
        &page,
        format!(
            "<< /Type /Page /Parent {pages} /MediaBox {} \
             /Resources << /Font << /F1 {font} >> >> /Contents {content} /Annots [{field}] >>",
            build::PDF_PAGE
        ),
    );
    pdf.set(
        &pages,
        format!("<< /Type /Pages /Kids [{page}] /Count 1 >>"),
    );
    pdf.set(
        &acroform,
        format!(
            "<< /Fields [{outermost}] /DA (/Helv 0 Tf 0 g) \
             /DR << /Font << /Helv {font} >> >> >>"
        ),
    );
    let catalog = pdf.push(format!(
        "<< /Type /Catalog /Pages {pages} /AcroForm {acroform} >>"
    ));
    pdf.finish(&catalog, "")
}

#[test]
fn placing_many_objects_past_the_model_costs_the_objects_they_are() {
    // The file names the format's ceiling in a reference, which is enough to make the search for a
    // free number fall back to the numbers below it. The search resumes where it stopped, so the
    // cost is the objects placed rather than their square: the quadratic search took about
    // twenty-eight seconds over this file in this profile, and the resuming one a fraction of a
    // second, so the bound below separates them with room for a loaded machine either way.
    let directory = tempfile::tempdir().expect("a temporary directory");
    let started = Instant::now();
    let outcome = rotate(
        directory.path(),
        &build::pdf_many_objects_past_the_model(12_800),
        "many",
    );
    produced(&outcome);
    assert!(
        started.elapsed().as_secs() < 15,
        "placing the objects took {:?}",
        started.elapsed()
    );
}

#[test]
fn a_new_object_does_not_take_a_number_a_dangling_reference_names() {
    // Object 201 is named by the file and holds nothing. A resize adds content streams; a number
    // given to one of them is one no reference the file states names, so 201 keeps naming nothing.
    let directory = tempfile::tempdir().expect("a temporary directory");
    let outcome = run(
        directory.path(),
        "pdf_resize",
        &build::pdf_dangling_reference_above_the_objects(),
        "dangling",
        json!({ "width": 400, "height": 500, "pages": "all" }),
    );
    let bytes = produced(&outcome);
    assert!(
        !bytes.windows(11).any(|window| window == b"\n201 0 obj\n"),
        "a new object took the number the dangling reference names"
    );
}

#[test]
fn a_page_tree_entry_stated_as_a_reference_to_a_reference_is_refused() {
    // The reference reads `/Kids` and `/Pages` with a typed lookup, which takes one step: what it
    // finds is a reference, so its load faults and the file is refused.
    let directory = tempfile::tempdir().expect("a temporary directory");
    for (fixture, name) in [
        (build::pdf_page_tree_kids_by_reference(), "kids"),
        (build::pdf_page_tree_root_by_reference(), "root"),
    ] {
        let outcome = rotate(directory.path(), &fixture, name);
        let failure = outcome.failure.expect("the file is refused");
        assert!(failure.is_usage(), "{name}: {failure:?}");
        assert!(
            failure.reason.contains("cannot read"),
            "{name}: {}",
            failure.reason
        );
    }
}

#[test]
fn a_box_entry_stated_as_a_reference_to_a_reference_faults() {
    // `/MediaBox` is read with a lookup that is not a `lookupMaybe`: a reference that names a
    // reference is not four numbers, so the reference's own type check faults inside the resize.
    let directory = tempfile::tempdir().expect("a temporary directory");
    let outcome = run(
        directory.path(),
        "pdf_resize",
        &build::pdf_media_box_by_reference(),
        "box",
        json!({ "width": 400, "height": 500, "pages": "all" }),
    );
    let failure = outcome.failure.expect("the call faults");
    assert!(!failure.is_usage(), "{failure:?}");
}

#[test]
fn a_null_crop_box_falls_back_to_the_media_box() {
    // `/CropBox` is read with a `lookupMaybe`, which answers "no crop" for a null: the page shows
    // its media box, and the crop rebuilds that box rather than faulting on the entry.
    let directory = tempfile::tempdir().expect("a temporary directory");
    let outcome = run(
        directory.path(),
        "pdf_crop",
        &build::pdf_crop_box_null(),
        "null-crop",
        json!({
            "area": { "x": 0.0, "y": 0.0, "width": 1.0, "height": 1.0 },
            "pages": "all",
        }),
    );
    let bytes = produced(&outcome);
    let box_entry = b"/CropBox[0 0 595.28 841.89]";
    assert!(
        bytes
            .windows(box_entry.len())
            .any(|window| window == box_entry),
        "the page shows its media box"
    );
}

#[test]
fn a_form_reached_through_a_chained_reference_faults() {
    // `/AcroForm` is read with a typed lookup that takes one step: a reference that names a
    // reference is not the form dictionary, so the reference's fill faults and so does this one.
    let directory = tempfile::tempdir().expect("a temporary directory");
    let outcome = run(
        directory.path(),
        "pdf_form_fill",
        &build::pdf_acroform_by_reference(),
        "chained-acroform",
        json!({ "values": { "name": "x" } }),
    );
    let failure = outcome.failure.expect("the call faults");
    assert!(!failure.is_usage(), "{failure:?}");
}

#[test]
fn a_field_parent_stated_as_a_reference_to_a_reference_faults() {
    // Ascending a field's `/Parent` chain reads each link with a typed lookup that takes one step:
    // what it finds here is a reference, not the dictionary it needs, so the reference's fill faults
    // and so does this one rather than leaving the field it cannot read out of the form.
    let directory = tempfile::tempdir().expect("a temporary directory");
    let outcome = run(
        directory.path(),
        "pdf_form_fill",
        &build::pdf_field_parent_by_reference(),
        "chained-parent",
        json!({ "values": { "name": "x" } }),
    );
    let failure = outcome.failure.expect("the call faults");
    assert!(!failure.is_usage(), "{failure:?}");
}

#[test]
fn a_fields_entry_stated_as_a_reference_to_a_reference_faults() {
    // The field tree is built before any value is applied, and every `/Fields` element is read with
    // the same one-step lookup: an element that is a reference to a reference is not a field, so the
    // reference's fill faults and so does this one rather than filling a form it could not walk.
    let directory = tempfile::tempdir().expect("a temporary directory");
    let outcome = run(
        directory.path(),
        "pdf_form_fill",
        &build::pdf_fields_by_reference(),
        "chained-fields",
        json!({ "values": { "name": "x" } }),
    );
    let failure = outcome.failure.expect("the call faults");
    assert!(!failure.is_usage(), "{failure:?}");
}

#[test]
fn a_page_whose_resources_name_a_reference_faults() {
    // Adding a font reads the page's `/Resources` with a typed lookup that takes one step: what it
    // finds is a reference, not a dictionary, so the draw faults rather than replacing entries it
    // never read.
    let directory = tempfile::tempdir().expect("a temporary directory");
    let outcome = run(
        directory.path(),
        "pdf_text",
        &build::pdf_resources_by_reference(),
        "chained-resources",
        json!({ "text": "x", "pages": "all" }),
    );
    let failure = outcome.failure.expect("the call faults");
    assert!(!failure.is_usage(), "{failure:?}");
}

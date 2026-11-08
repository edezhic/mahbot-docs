//! The engine crate's contract, proved from the harness side: its vocabulary is the reference's
//! own data, its outcome and reading shapes carry every field the baseline records under the same
//! names, and its closed operation set holds every operation and edit sub-operation the corpus
//! exercises.

use document_engine::operation::{Destination, Failure, Outcome, Request, execute};
use document_engine::reading;
use document_engine::vocabulary;
use document_parity::baseline::Baseline;
use document_parity::contract;
use document_parity::corpus;
use document_parity::transcript::CaseTranscript;
use serde_json::Value;
use std::collections::BTreeSet;
use std::path::Path;

/// The recorded baseline, exactly as it is stored.
fn baseline_json() -> Value {
    let path = Baseline::default_path();
    let bytes = std::fs::read(&path)
        .unwrap_or_else(|e| panic!("cannot read the baseline at {}: {e}", path.display()));
    serde_json::from_slice(&bytes).expect("the baseline is valid JSON")
}

/// The strings under `key`, or none when the transcript leaves the field out.
fn strings(case: &Value, key: &str) -> Vec<String> {
    case.get(key)
        .and_then(Value::as_array)
        .map(|values| {
            values
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
}

#[test]
fn the_vocabulary_is_the_references_own_statement_of_the_same_data() {
    let vocabulary = vocabulary::vocabulary();
    assert_eq!(
        &vocabulary::json()["rules"],
        contract::reference_rules(),
        "the crate's rules section must be the reference's rules.json"
    );
    assert_eq!(
        vocabulary.reading,
        contract::reference_reading(),
        "every reading bound must be the reference's"
    );
    assert_eq!(
        vocabulary.limits,
        contract::reference_limits(),
        "every reader and producer cap must be the reference's"
    );
    assert_eq!(
        sha256(vocabulary::FONT),
        sha256(contract::reference_font()),
        "the crate's font must be the reference's own bytes"
    );

    let marks = &vocabulary.marks;
    for mark in contract::reference_marks() {
        let actual = if mark.values.is_empty() {
            marks.get(mark.key).to_string()
        } else {
            marks.format(mark.key, mark.values)
        };
        assert_eq!(actual, mark.rendered, "mark `{}`", mark.key);
    }
    for (key, line) in contract::reference_unshown() {
        assert_eq!(
            marks.format("unshown_line", &[("count", "1"), ("what", marks.get(key))]),
            line,
            "unshown noun `{key}`"
        );
    }
    // The mark check runs one way: every mark the reference prints must be in the vocabulary. A
    // mark the vocabulary adds with no reference producer (an engine reason) is not compared. The
    // fixed note sentences are the one closed class where the other direction is checked too,
    // because a note the vocabulary adds alone would be prose the engine never prints. Their keys
    // name the reader that prints them; the `docx_note_*` marks are templates, not fixed notes.
    let is_note = |key: &str| {
        key.starts_with("pdf_note_")
            || key.starts_with("xlsx_note_")
            || key.starts_with("legacy_note_")
    };
    let vocabulary_notes: BTreeSet<&str> = vocabulary::json()["marks"]
        .as_object()
        .expect("the vocabulary states its marks")
        .keys()
        .map(String::as_str)
        .filter(|key| is_note(key))
        .collect();
    let reference_notes: BTreeSet<&str> = contract::reference_marks()
        .iter()
        .map(|mark| mark.key)
        .filter(|key| is_note(key))
        .collect();
    assert_eq!(vocabulary_notes, reference_notes, "the note sentences");
}

#[test]
fn every_recorded_reading_round_trips_through_the_engine_shape() {
    let baseline = baseline_json();
    let readings = baseline["readings"]
        .as_object()
        .expect("the baseline records readings");
    assert!(!readings.is_empty());
    for (document, entry) in readings {
        let value = entry["reading"].clone();
        let reading: reading::Reading = serde_json::from_value(value.clone()).unwrap_or_else(|e| {
            panic!("{document}: the reading does not fit the engine's shape: {e}")
        });
        assert_eq!(
            serde_json::to_value(&reading).expect("a reading serialises"),
            value,
            "{document}"
        );
    }
}

#[test]
fn every_recorded_outcome_round_trips_through_the_engine_shape() {
    let baseline = baseline_json();
    let cases = baseline["cases"]
        .as_object()
        .expect("the baseline records cases");
    assert!(!cases.is_empty());
    for (slug, case) in cases {
        let failure = case
            .get("class")
            .and_then(Value::as_str)
            .map(|class| Failure {
                class: class.to_string(),
                reason: case
                    .get("reason")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string(),
            });
        let outcome = Outcome {
            outputs: Vec::new(),
            missing: strings(case, "missing"),
            placeholders: case
                .get("placeholders")
                .and_then(Value::as_u64)
                .map(|count| count as usize),
            unsupported: strings(case, "unsupported"),
            notes: strings(case, "notes"),
            losses: strings(case, "losses"),
            failure,
        };
        let rebuilt = CaseTranscript::build(&outcome, |_| unreachable!("no output is named"))
            .expect("an outcome builds a transcript");
        let mut actual = serde_json::to_value(&rebuilt).expect("a transcript serialises");
        actual
            .as_object_mut()
            .expect("a transcript is an object")
            .remove("outputs");
        let mut expected = case.clone();
        expected
            .as_object_mut()
            .expect("a transcript is an object")
            .remove("outputs");
        assert_eq!(actual, expected, "{slug}");
    }
}

#[test]
fn the_outcome_carries_the_recorded_field_names() {
    // The baseline records the fields the reference's answer carries; a field it gains must be a
    // field the engine's outcome carries.
    let baseline = baseline_json();
    let recorded: BTreeSet<&str> = baseline["cases"]
        .as_object()
        .expect("the baseline records cases")
        .values()
        .flat_map(|case| {
            case.as_object()
                .expect("a case is an object")
                .keys()
                .map(String::as_str)
        })
        .filter(|key| !matches!(*key, "status" | "class" | "reason"))
        .collect();
    let shape = serde_json::to_value(Outcome::default()).expect("an outcome serialises");
    let keys: BTreeSet<&str> = shape
        .as_object()
        .expect("an outcome is an object")
        .keys()
        .map(String::as_str)
        .collect();
    assert!(
        recorded.is_subset(&keys),
        "the outcome is missing {recorded:?}"
    );
    // The two fields the recorded corpus never exercises are part of the frozen shape too.
    assert_eq!(
        keys,
        BTreeSet::from([
            "outputs",
            "missing",
            "placeholders",
            "unsupported",
            "notes",
            "losses",
        ]),
        "a produced outcome carries these fields and no others"
    );
    assert_eq!(shape["outputs"], serde_json::json!([]));
    assert_eq!(shape["placeholders"], Value::Null);
    let failed =
        serde_json::to_value(Outcome::failed("usage", "no")).expect("a failure serialises");
    assert_eq!(failed["failure"]["class"], "usage");
    assert_eq!(failed["failure"]["reason"], "no");
}

#[test]
fn every_corpus_operation_and_edit_is_in_the_engines_closed_set() {
    let root = tempfile::TempDir::new().expect("a temporary destination");
    let mut operations = BTreeSet::new();
    let mut edits = BTreeSet::new();
    for (index, case) in corpus::writer_cases().into_iter().enumerate() {
        // Each case gets its own directory: once the operations write, two cases with the same
        // base must not collide on an existing output.
        let directory = root.path().join(format!("case-{index}"));
        std::fs::create_dir(&directory).expect("a case directory");
        let mut values = case
            .request
            .as_object()
            .cloned()
            .expect("a case's request is an object");
        // The destination carries the outputs; a value must not.
        values.remove("op");
        values.remove("output");
        values.remove("outputs");
        let request = Request {
            operation: case.operation.to_string(),
            values,
            destination: Destination {
                directory,
                base_name: "out".to_string(),
            },
        };
        let outcome = execute(&request);
        // Recognition, not execution: an operation the engine does not know answers that its name
        // is unknown, and an edit sub-operation it does not know answers the same about the edit.
        // Every other answer — a refusal of an unresolved value included, such as `unknown
        // format` — means the name is in the closed set, which is what this test pins. The two
        // sentences are read from the vocabulary the engine prints from, so the check tracks the
        // engine's wording instead of restating it and a rewording cannot make it vacuous.
        if let Some(failure) = outcome.failure {
            let family = case.operation.strip_suffix("_edit").unwrap_or_default();
            let marks = &vocabulary::vocabulary().marks;
            let unknown_operation =
                marks.format("surface_unknown_operation", &[("name", case.operation)]);
            let unknown_edit = case.sub_operation.map(|sub| {
                marks.format("surface_unknown_edit", &[("family", family), ("name", sub)])
            });
            assert!(
                failure.reason != unknown_operation
                    && unknown_edit.as_deref() != Some(failure.reason.as_str()),
                "{} is not recognised: {}",
                case.key(),
                failure.reason
            );
        }
        operations.insert(case.operation);
        if let Some(sub) = case.sub_operation
            && case.expect == corpus::Expect::Produced
            && matches!(case.operation, "docx_edit" | "xlsx_edit" | "pptx_edit")
        {
            edits.insert((case.operation, sub));
        }
    }
    assert_eq!(operations.len(), 13, "the corpus covers every operation");
    assert_eq!(
        edits.len(),
        37,
        "the corpus covers every edit sub-operation"
    );
}

/// The SHA-256 of `bytes`, as the crate reports an artifact's content.
fn sha256(bytes: &[u8]) -> String {
    use sha2::Digest as _;
    format!("{:x}", sha2::Sha256::digest(bytes))
}

/// The reference reader's sources are copies, never links: this is what makes a change to the
/// product's sources fail the gate instead of passing unnoticed, and what lets `cargo fmt` here
/// run without ever touching a file this crate does not own.
#[test]
fn the_reference_sources_are_byte_for_byte_the_products() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let product = root
        .parent()
        .expect("the harness lives inside the workspace");
    for relative in [
        "document.rs",
        "ooxml.rs",
        "ooxml/docx.rs",
        "ooxml/xlsx.rs",
        "ooxml/chart.rs",
        "ooxml/numfmt.rs",
        "legacy.rs",
        "pdf_marks.rs",
        "reader_output.rs",
    ] {
        let copied = root.join("src/reference_sources").join(relative);
        let original = product.join("src").join(relative);
        let copied_bytes = std::fs::read(&copied)
            .unwrap_or_else(|e| panic!("cannot read {}: {e}", copied.display()));
        let original_bytes = std::fs::read(&original)
            .unwrap_or_else(|e| panic!("cannot read {}: {e}", original.display()));
        assert!(
            copied_bytes == original_bytes,
            "src/reference_sources/{relative} is not byte-for-byte the product's src/{relative} — \
             re-copy it deliberately"
        );
    }
}

/// The host's copies of the product's media tables and raster bounds are the product's own values,
/// not hand-copied numbers that can drift.
#[test]
fn the_host_media_tables_and_raster_limits_are_the_products_own() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let workspace = root
        .parent()
        .expect("the harness lives inside the workspace");
    let util = std::fs::read_to_string(workspace.join("product-touchpoints/src/util/mod.rs"))
        .expect("the product's util module");
    let media =
        std::fs::read_to_string(workspace.join("product-touchpoints/src/util/media_target.rs"))
            .expect("the product's media-target module");

    let host = |values: &[&str]| -> Vec<String> {
        values.iter().map(|value| (*value).to_string()).collect()
    };
    assert_eq!(
        host(contract::host_video_extensions()),
        contract::string_list(&util, "VIDEO_EXTENSIONS"),
        "the host's video list is not the product's"
    );
    assert_eq!(
        host(contract::host_image_extensions()),
        contract::string_list(&util, "IMAGE_EXTENSIONS"),
        "the host's image list is not the product's"
    );
    assert_eq!(
        contract::host_raster_decode_max_alloc_bytes(),
        contract::integer(&media, "RASTER_DECODE_MAX_ALLOC_BYTES"),
        "the host's raster allocation bound is not the product's"
    );
    assert_eq!(
        contract::host_raster_decode_max_dimension_px(),
        contract::integer(&media, "RASTER_DECODE_MAX_DIMENSION_PX") as u32,
        "the host's raster dimension bound is not the product's"
    );
    // The crate's own media tables are the product's too: a drift there would only surface
    // indirectly through the corpus.
    let support = std::fs::read_to_string(workspace.join("document-engine/src/support.rs"))
        .expect("the crate's support module");
    assert_eq!(
        contract::string_list(&support, "VIDEO_EXTENSIONS"),
        contract::string_list(&util, "VIDEO_EXTENSIONS"),
        "the crate's video list is not the product's"
    );
    assert_eq!(
        contract::string_list(&support, "IMAGE_EXTENSIONS"),
        contract::string_list(&util, "IMAGE_EXTENSIONS"),
        "the crate's image list is not the product's"
    );
}

/// The codec versions are pinned in the manifests and recorded in every engine identity. A
/// dependency bump changes every compared digest, so it must change the identity the reference arm
/// checks rather than pass as "identity matches".
#[test]
fn the_identities_record_the_codec_versions() {
    use document_parity::engine::Reader as _;

    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let workspace = root
        .parent()
        .expect("the harness lives inside the workspace");
    let recorded = env!("PARITY_CODEC_VERSIONS");
    assert!(!recorded.is_empty(), "the build records the codec versions");

    // The list has one home — `document_parity::codecs`, which the build script includes — so a
    // codec added there is automatically held to being recorded rather than needing a second copy.
    for name in document_parity::codecs::CODECS {
        assert!(
            recorded
                .split(';')
                .any(|entry| entry.starts_with(&format!("{name}="))),
            "the identity does not record `{name}`"
        );
    }

    // The recorded string is the lockfile's own answer, so the identity cannot name a version
    // other than the one compiled.
    let lock =
        std::fs::read_to_string(workspace.join("Cargo.lock")).expect("the workspace lockfile");
    assert_eq!(
        recorded,
        document_parity::codecs::codec_versions(&lock),
        "the recorded codec versions are not the lockfile's"
    );

    // Every arm that decodes a compared byte carries the digest: both readers and the writer.
    let digest = sha256(recorded.as_bytes());
    let reference = document_parity::reference::ReferenceReader;
    let crate_reader = document_parity::reference::CrateReader;
    for identity in [reference.identity(), crate_reader.identity()] {
        assert_eq!(
            identity.components.get("codecs"),
            Some(&digest),
            "`{}` does not record the codec versions",
            identity.engine
        );
    }
    let baseline = baseline_json();
    assert_eq!(
        baseline["writer"]["components"]["codecs"].as_str(),
        Some(digest.as_str()),
        "the recorded writer identity does not record the codec versions"
    );

    // Every codec in `CODECS` that a manifest names is pinned exactly, so a patch bump is a
    // deliberate edit rather than a caret range the lock can move under the identity. A codec a
    // manifest adds must join `CODECS` to be recorded at all; this loop then holds its pin.
    let manifests: Vec<(&str, String)> = ["document-engine/Cargo.toml", "parity/Cargo.toml"]
        .into_iter()
        .map(|manifest| {
            let text = std::fs::read_to_string(workspace.join(manifest)).expect("a manifest");
            (manifest, text)
        })
        .collect();
    for codec in document_parity::codecs::CODECS {
        for (manifest, text) in &manifests {
            for line in text.lines() {
                let line = line.trim_start();
                if line.starts_with(&format!("{codec} = ")) {
                    assert!(
                        line.contains("\"="),
                        "`{manifest}` must pin `{codec}` exactly"
                    );
                }
            }
        }
    }
}

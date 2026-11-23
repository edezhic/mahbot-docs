//! The comparison's own invariants, tested without PDFium and without LibreOffice.
//!
//! The fixed set and its controls are exercised through the engine's renderer alone; the rasteriser
//! digest check, the stale-decision check and the determinism check need neither outside tool. A
//! machine with no PDFium therefore still runs everything here, which is what makes the comparison
//! safe to add to the package's test set.

use std::collections::BTreeMap;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use document_engine::vocabulary::Family;

use crate::oracle::TempArea;
use crate::oracle::chain::Assets;
use crate::oracle::compare::cases::{ASSETS, CASES, Class};
use crate::oracle::compare::decisions::{Category, Decision, Decisions};
use crate::oracle::compare::metric::{
    Attribution, Calibration, DivergenceKind, compare_document, controls,
};
use crate::oracle::compare::render::{Drawn, SideDocument, SideState, render_engine};
use crate::oracle::compare::report::Identity;
use crate::oracle::compare::{Error, Options, Prepared, Ran, Rendered, compare, run};
use crate::oracle::pdfium::Rasteriser;

/// The committed calibration.
fn calibration() -> Calibration {
    serde_json::from_str(include_str!("../compare/calibration.json")).unwrap()
}

/// The committed rasteriser record.
fn rasteriser() -> Rasteriser {
    serde_json::from_str(include_str!("../compare/rasteriser.json")).unwrap()
}

/// A prepared run for the tests: the committed records and the decisions the test states.
fn prepared(decisions: Decisions) -> Prepared {
    let calibration = calibration();
    let record = rasteriser();
    let identity = Identity::new(
        record.clone(),
        &calibration,
        crate::oracle::canonical::digest(include_bytes!("../compare/calibration.json")),
        crate::oracle::canonical::digest(include_bytes!("../compare/decisions.json")),
    );
    Prepared {
        record,
        library: PathBuf::new(),
        helper: crate::oracle::pdfium::Helper::ThisBinary,
        calibration,
        decisions,
        identity,
    }
}

#[test]
fn the_committed_records_are_consistent() {
    let calibration = calibration();
    calibration.validate().unwrap();
    let rasteriser = rasteriser();
    assert_eq!(calibration.pdfium_release, rasteriser.release);
    assert_eq!(calibration.pdfium_sha256, rasteriser.sha256);
    for digest in [&rasteriser.sha256, &rasteriser.archive_sha256] {
        assert!(
            crate::oracle::pdfium::is_digest(digest),
            "a committed digest is 64 lowercase hexadecimal digits: {digest}"
        );
    }
    assert!(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("dev/oracle/compare")
            .join(&rasteriser.licence)
            .is_file()
    );
    // A committed decision is allowed: the test checks that the file reads and that each key names
    // a family, a document, a page or `document`, and a kind — not that the file is empty, and not
    // how many segments the document's own name holds.
    let (decisions, _) = Decisions::read_with_digest(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("dev/oracle/compare"),
    )
    .unwrap();
    for decision in &decisions.decisions {
        let key = decision.key.as_str();
        let mut parts = key.split('/');
        let family = parts.next().unwrap_or_default();
        let kind = parts.next_back().unwrap_or_default();
        let page = parts.next_back().unwrap_or_default();
        let name = parts.collect::<Vec<_>>().join("/");
        assert!(
            Family::ALL.iter().any(|known| known.as_str() == family),
            "{key}"
        );
        assert!(!name.is_empty() && name != "document", "{key}");
        assert!(page == "document" || page.parse::<u32>().is_ok(), "{key}");
        assert!(!kind.is_empty(), "{key}");
    }
}

#[test]
fn a_different_library_digest_refuses() {
    let area = TempArea::new("compare-digest").unwrap();
    let library = area.path().join("libpdfium.dylib");
    std::fs::write(&library, b"not the recorded PDFium").unwrap();
    let mut options = Options::new();
    options.evidence = area.path().join("evidence");
    options.library = Some(library);
    match run(&options, &mut Vec::new()) {
        Err(Error::Refused(message)) => {
            assert!(
                message.contains("not the one this comparison records"),
                "{message}"
            );
        }
        other => panic!("expected a refusal, got {other:?}"),
    }
}

#[test]
fn a_malformed_rasteriser_digest_is_refused() {
    let area = TempArea::new("compare-rasteriser").unwrap();
    let mut record = rasteriser();
    record.sha256 = "é".repeat(32);
    std::fs::write(
        area.path().join("rasteriser.json"),
        serde_json::to_vec(&record).unwrap(),
    )
    .unwrap();
    let mut options = Options::new();
    options.root = area.path().to_path_buf();
    match run(&options, &mut Vec::new()) {
        Err(Error::Refused(message)) => {
            assert!(message.contains("no usable sha256"), "{message}");
        }
        other => panic!("expected a refusal, got {other:?}"),
    }
}

#[test]
fn a_missing_library_fails_the_mechanism() {
    let area = TempArea::new("compare-missing").unwrap();
    let mut options = Options::new();
    options.evidence = area.path().join("evidence");
    options.library = Some(area.path().join("absent.dylib"));
    match run(&options, &mut Vec::new()) {
        Err(Error::Failed(message)) => assert!(message.contains("not at"), "{message}"),
        other => panic!("expected a mechanism failure, got {other:?}"),
    }
}

#[test]
fn a_foreign_evidence_directory_refuses_before_the_library_is_located() {
    // The claim runs first, so a run that would also fail on a missing library refuses (exit 2)
    // rather than failing (exit 1): an invocation that cannot mean anything never gets as far as
    // judging whether the mechanism works.
    let area = TempArea::new("compare-claim-first").unwrap();
    let directory = area.path().join("evidence");
    std::fs::create_dir_all(&directory).unwrap();
    std::fs::write(directory.join("notes.txt"), "someone else's").unwrap();
    let mut options = Options::new();
    options.evidence = directory.clone();
    options.library = Some(area.path().join("absent.dylib"));
    match run(&options, &mut Vec::new()) {
        Err(Error::Refused(message)) => {
            assert!(message.contains("not an evidence directory"), "{message}");
        }
        other => panic!("a foreign evidence directory must refuse first, got {other:?}"),
    }
}

#[test]
fn an_explicit_evidence_directory_survives_either_argument_order() {
    let forward = Options::parse([
        "--root".to_string(),
        "/tmp/root".to_string(),
        "--evidence".to_string(),
        "/tmp/evidence".to_string(),
    ])
    .unwrap();
    let reverse = Options::parse([
        "--evidence".to_string(),
        "/tmp/evidence".to_string(),
        "--root".to_string(),
        "/tmp/root".to_string(),
    ])
    .unwrap();
    assert_eq!(forward.evidence, Path::new("/tmp/evidence"));
    assert_eq!(reverse.evidence, Path::new("/tmp/evidence"));
    assert_eq!(forward.root, Path::new("/tmp/root"));
    assert_eq!(reverse.root, Path::new("/tmp/root"));
    let default = Options::parse(["--root".to_string(), "/tmp/root".to_string()]).unwrap();
    assert_eq!(default.evidence, Path::new("/tmp/root/evidence"));
}

#[test]
fn an_empty_or_flag_argument_is_refused() {
    for flag in ["--root", "--evidence", "--library", "--helper"] {
        for value in [String::new(), "--measure".to_string()] {
            assert!(
                Options::parse([flag.to_string(), value.clone()]).is_err(),
                "{flag} accepted `{value}` as a value"
            );
        }
    }
}

#[test]
fn a_stale_decision_is_reported_and_a_live_one_is_used() {
    let decisions = Decisions {
        schema: 1,
        decisions: vec![Decision {
            key: "pdf/text/1/pixels".to_string(),
            category: Category::OutsideQuirk,
            reason: "a decision for this test".to_string(),
        }],
    };
    let (used, stale) = decisions.check(&BTreeSet::new());
    assert!(used.is_empty());
    assert_eq!(stale, ["pdf/text/1/pixels"]);
    let (used, stale) = decisions.check(&BTreeSet::from(["pdf/text/1/pixels".to_string()]));
    assert_eq!(used.len(), 1);
    assert!(stale.is_empty());
}

#[test]
fn the_controls_fire_on_the_engine_page_set_without_an_outside_tool() {
    let calibration = calibration();
    let area = TempArea::new("compare-controls").unwrap();
    let assets = Assets::write(&area.path().join("assets"), ASSETS).unwrap();
    let mut built = Vec::new();
    for case in CASES {
        let document = crate::oracle::compare::cases::build(case, area.path(), &assets).unwrap();
        let engine = render_engine(&document.path, &calibration);
        assert_eq!(engine.state, SideState::Opened, "{}", case.id);
        built.push((case, engine));
    }
    let documents: Vec<Rendered<'_>> = built
        .iter()
        .map(|(case, engine)| (case.id, case.family, case.class, engine))
        .collect();
    let controls = controls(&documents, &calibration);
    assert_eq!(controls.len(), 6);
    for control in &controls {
        assert!(control.fired, "{}: {}", control.name, control.detail);
    }
}

#[test]
fn the_fixed_set_carries_the_required_shapes() {
    let calibration = calibration();
    let area = TempArea::new("compare-shapes").unwrap();
    let assets = Assets::write(&area.path().join("assets"), ASSETS).unwrap();
    let mut pages = BTreeMap::new();
    let mut produced = BTreeMap::new();
    for case in CASES {
        let document = crate::oracle::compare::cases::build(case, area.path(), &assets).unwrap();
        let engine = render_engine(&document.path, &calibration);
        pages.insert(case.id, engine);
        produced.insert(case.id, document.bytes);
    }
    // A non-standard 300-point square page, at the long side both sides are asked for.
    assert_eq!(crate::oracle::compare::cases::LARGEST_PAGE_POINTS, 300);
    let small = pages["pdf/small"]
        .pages
        .first()
        .and_then(|page| page.drawn.dimensions())
        .expect("the small page must draw");
    assert_eq!(small.width, small.height);
    assert_eq!(small.width, calibration.metric.long_side_px);
    // Several pages, and a page carrying ink.
    assert!(pages["pdf/pages"].pages.len() >= 2);
    assert!(
        pages
            .values()
            .flat_map(|document| &document.pages)
            .any(|page| { page.drawn.has_ink(calibration.metric.ink_luminance) })
    );
    // The annotation the engine's `pdf_text` preserves and moves is in the judged file.
    assert!(
        produced["pdf/annotated"]
            .windows(7)
            .any(|window| window == b"/Annots")
    );
}

#[test]
fn a_calibration_with_an_unusable_number_is_refused() {
    let mut unusable = calibration();
    unusable.metric.ink_luminance = 0.0;
    assert!(unusable.validate().is_err());

    let mut unusable = calibration();
    unusable.metric.max_scale = 1.0;
    assert!(unusable.validate().is_err());

    let mut unusable = calibration();
    unusable.metric.dimension_rounding_px = 0;
    assert!(unusable.validate().is_err());

    let mut unusable = calibration();
    unusable.pdfium_sha256 = unusable.pdfium_sha256.to_uppercase();
    let error = unusable.validate().unwrap_err();
    assert!(
        error.contains("no usable rasteriser digest"),
        "an uppercase digest must name itself, not read as another release: {error}"
    );
}

#[test]
fn an_engine_unreadable_document_is_an_engine_failure() {
    let calibration = calibration();
    let area = TempArea::new("compare-engine-unreadable").unwrap();
    let assets = Assets::write(&area.path().join("assets"), ASSETS).unwrap();
    let case = CASES
        .iter()
        .find(|case| case.id == "pdf/text")
        .expect("the fixed set carries the text case");
    let document = crate::oracle::compare::cases::build(case, area.path(), &assets).unwrap();
    let outside = render_engine(&document.path, &calibration);
    let engine = SideDocument::unreadable("the engine says no".to_string());
    let outcome = compare_document(
        "pdf/text",
        Family::Pdf,
        Class::Text,
        &engine,
        &outside,
        &BTreeSet::new(),
        &calibration,
    );
    assert_eq!(outcome.divergences.len(), 1);
    assert_eq!(outcome.divergences[0].kind, DivergenceKind::EngineFailed);
    assert_eq!(outcome.divergences[0].attribution, Attribution::Engine);
    assert_eq!(
        outcome.divergences[0].key,
        "pdf/text/document/engine-failed"
    );
    assert!(outcome.divergences[0].page.is_none());
    assert!(outcome.mechanism_failures.is_empty());
    assert!(outcome.not_compared.is_empty());
}

/// Every file under `root`, relative to it and sorted, for a byte-for-byte comparison.
fn evidence(root: &Path) -> Vec<(String, Vec<u8>)> {
    fn walk(root: &Path, directory: &Path, files: &mut Vec<(String, Vec<u8>)>) {
        for entry in std::fs::read_dir(directory).expect("the evidence directory must be readable")
        {
            let path = entry.expect("an evidence entry").path();
            if path.is_dir() {
                walk(root, &path, files);
            } else {
                let relative = path
                    .strip_prefix(root)
                    .expect("an evidence file under its root")
                    .to_string_lossy()
                    .replace('\\', "/");
                files.push((relative, std::fs::read(&path).expect("an evidence file")));
            }
        }
    }
    let mut files = Vec::new();
    walk(root, root, &mut files);
    files.sort();
    files
}

/// How the substitute outside renderer answers.
#[derive(Clone, Copy, PartialEq)]
enum Outside {
    /// It draws exactly what the engine drew.
    Same,
    /// It halves the first page, which diverges.
    Diverge,
    /// It aborts on the first document, with a reason naming the file it was given.
    Abort,
}

/// Drive the whole comparison pipeline — build, render, compare, report, evidence — with the
/// engine's own rendering standing in for the outside tool. `Diverge` halves the first page's
/// dimensions and records a decision for it, so the report carries a divergence, a used decision and
/// evidence images; `Abort` makes one document not compared, with a reason carrying a machine path.
fn run_comparison(area: &TempArea, outside: Outside) -> (String, Vec<(String, Vec<u8>)>) {
    let calibration = calibration();
    let decisions = Decisions {
        schema: 1,
        decisions: if outside == Outside::Diverge {
            vec![Decision {
                key: format!("{}/1/dimension", CASES[0].id),
                category: Category::EngineDefect,
                reason: "a decision this test records".to_string(),
            }]
        } else {
            Vec::new()
        },
    };
    let prepared = prepared(decisions);
    let mut options = Options::new();
    options.evidence = area.path().join("evidence");
    let render = |case: &crate::oracle::compare::cases::Case, document: &Path, _: &Path| {
        if outside == Outside::Abort && case.id == CASES[0].id {
            return SideDocument {
                pages: Vec::new(),
                page_count: None,
                state: SideState::Aborted(format!("the helper failed on {}", document.display())),
            };
        }
        let mut side = render_engine(document, &calibration);
        if outside == Outside::Diverge
            && case.id == CASES[0].id
            && let Some(page) = side.pages.first_mut()
            && let Drawn::Picture { width, height, .. } = &page.drawn
        {
            let width = (*width / 2).max(1);
            let height = (*height / 2).max(1);
            page.drawn = Drawn::Picture {
                width,
                height,
                rgba: vec![255; (width * height * 4) as usize],
            };
        }
        side
    };
    let mut out = Vec::new();
    let summary = match compare(&options, &prepared, &mut out, render).unwrap() {
        Ran::Compared(summary) => summary,
        Ran::Measured => panic!("a comparison must compare"),
    };
    assert!(!summary.failed, "{summary:?}");
    (String::from_utf8(out).unwrap(), evidence(&options.evidence))
}

#[test]
fn a_measure_run_that_cannot_measure_every_page_fails() {
    let area = TempArea::new("compare-measure-partial").unwrap();
    let prepared = prepared(Decisions {
        schema: 1,
        decisions: Vec::new(),
    });
    let mut options = Options::new();
    options.evidence = area.path().join("evidence");
    options.measure = true;
    let calibration = calibration();
    let render = |case: &crate::oracle::compare::cases::Case, document: &Path, _: &Path| {
        if case.id == CASES[0].id {
            SideDocument::unreadable("the outside tool did not open it".to_string())
        } else {
            render_engine(document, &calibration)
        }
    };
    let mut out = Vec::new();
    match compare(&options, &prepared, &mut out, render) {
        Err(Error::Failed(message)) => {
            assert!(message.contains(CASES[0].id), "{message}");
        }
        other => panic!("a partial measurement must fail, got {other:?}"),
    }
    assert!(
        !options.evidence.exists(),
        "a failed measurement must write no calibration data"
    );
}

#[test]
fn a_measure_run_that_cannot_measure_the_engine_side_fails() {
    let calibration = calibration();
    let engine = SideDocument::unreadable("the engine says no".to_string());
    let outside = SideDocument {
        pages: Vec::new(),
        page_count: Some(0),
        state: SideState::Opened,
    };
    let engine: [Rendered<'_>; 1] = [("pdf/text", Family::Pdf, Class::Text, &engine)];
    let outside: [Rendered<'_>; 1] = [("pdf/text", Family::Pdf, Class::Text, &outside)];
    let mut out = Vec::new();
    let error = crate::oracle::compare::measure::measure(&engine, &outside, &calibration, &mut out)
        .unwrap_err();
    assert!(error.contains("pdf/text"), "{error}");
    assert!(error.contains("the engine"), "{error}");
    assert!(out.is_empty(), "a failed measurement must print nothing");
}

#[test]
fn a_foreign_evidence_directory_refuses() {
    let area = TempArea::new("compare-foreign-evidence").unwrap();
    let directory = area.path().join("evidence");
    std::fs::create_dir_all(&directory).unwrap();
    std::fs::write(directory.join("notes.txt"), "someone else's").unwrap();
    let prepared = prepared(Decisions {
        schema: 1,
        decisions: Vec::new(),
    });
    let mut options = Options::new();
    options.evidence = directory.clone();
    let calibration = calibration();
    let render = |_: &crate::oracle::compare::cases::Case, document: &Path, _: &Path| {
        render_engine(document, &calibration)
    };
    let mut out = Vec::new();
    match compare(&options, &prepared, &mut out, render) {
        Err(Error::Refused(message)) => {
            assert!(message.contains("not an evidence directory"), "{message}");
        }
        other => panic!("a foreign evidence directory must refuse, got {other:?}"),
    }
    assert_eq!(
        std::fs::read_to_string(directory.join("notes.txt")).unwrap(),
        "someone else's"
    );
}

#[test]
fn a_measurement_does_not_require_the_calibration_to_agree_with_the_record() {
    // `--measure` is the mode that writes a new calibration, so it must run against the release the
    // record now names while the calibration still names the old one; a comparison refuses instead.
    let area = TempArea::new("compare-upgrade").unwrap();
    let library = area.path().join("libpdfium.dylib");
    std::fs::write(&library, b"a freshly fetched release").unwrap();
    let helper = area.path().join("rasteriser-helper");
    std::fs::write(&helper, b"a helper").unwrap();
    let mut record = rasteriser();
    record.release = "chromium/9999".to_string();
    record.sha256 = crate::oracle::pdfium::digest(&library).unwrap();
    record.archive_sha256 = "1".repeat(64);
    let mut calibration: serde_json::Value =
        serde_json::from_str(include_str!("../compare/calibration.json")).unwrap();
    calibration["pdfium_release"] = "chromium/1".into();
    calibration["pdfium_sha256"] = "2".repeat(64).into();
    let root = area.path().join("compare");
    std::fs::create_dir_all(&root).unwrap();
    for (name, bytes) in [
        ("rasteriser.json", serde_json::to_vec(&record).unwrap()),
        (
            "calibration.json",
            serde_json::to_vec(&calibration).unwrap(),
        ),
        (
            "decisions.json",
            std::fs::read(
                Path::new(env!("CARGO_MANIFEST_DIR")).join("dev/oracle/compare/decisions.json"),
            )
            .unwrap(),
        ),
    ] {
        std::fs::write(root.join(name), bytes).unwrap();
    }
    let mut options = Options::new();
    options.root = root;
    options.evidence = area.path().join("evidence");
    options.library = Some(library);
    options.helper = Some(helper);
    options.measure = true;
    assert!(
        crate::oracle::compare::prepare(&options).is_ok(),
        "a measurement must run against the release the record names"
    );
    options.measure = false;
    let error = match crate::oracle::compare::prepare(&options) {
        Ok(_) => panic!("a comparison must refuse a calibration of another build"),
        Err(error) => error,
    };
    match error {
        Error::Refused(message) => assert!(
            message.contains("the calibration was measured with"),
            "{message}"
        ),
        Error::Failed(message) => panic!("expected a refusal, got a failure: {message}"),
    }
}

#[test]
fn a_second_run_gives_the_same_report_and_evidence() {
    for outside in [Outside::Same, Outside::Diverge, Outside::Abort] {
        let first = TempArea::new("compare-determinism-a").unwrap();
        let second = TempArea::new("compare-determinism-b").unwrap();
        let (first_out, first_files) = run_comparison(&first, outside);
        let (second_out, second_files) = run_comparison(&second, outside);
        assert_eq!(first_out, second_out, "stdout differs");
        assert_eq!(first_files, second_files, "report or evidence differs");
        let report: serde_json::Value = serde_json::from_slice(
            &std::fs::read(first.path().join("evidence/report.json")).unwrap(),
        )
        .unwrap();
        if outside == Outside::Diverge {
            assert_eq!(report["decisions"]["used"].as_array().unwrap().len(), 1);
            assert!(report["decisions"]["stale"].as_array().unwrap().is_empty());
            assert!(!report["evidence"].as_array().unwrap().is_empty());
        }
        if outside == Outside::Abort {
            // A reason carrying a path must reach the report as `<machine>`, so two runs from
            // different temporary areas report the same bytes.
            let reasons = report["families"]
                .as_array()
                .unwrap()
                .iter()
                .flat_map(|family| family["documents"].as_array().unwrap())
                .flat_map(|document| document["not_compared"].as_array().unwrap())
                .map(|missing| missing["reason"].as_str().unwrap())
                .collect::<Vec<_>>();
            assert!(
                reasons.iter().any(|reason| reason.contains("<machine>")),
                "{reasons:?}"
            );
        }
    }
}

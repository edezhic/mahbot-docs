//! The comparison's own invariants, tested without PDFium and without LibreOffice.
//!
//! The fixed set and its controls are exercised through the engine's renderer alone; the rasteriser
//! digest check, the stale-decision check and the determinism check need neither outside tool. A
//! machine with no PDFium therefore still runs everything here, which is what makes the comparison
//! safe to add to the package's test set.

use std::collections::BTreeMap;
use std::collections::BTreeSet;
use std::path::Path;

use document_engine::vocabulary::Family;

use crate::oracle::TempArea;
use crate::oracle::chain::Assets;
use crate::oracle::compare::cases::{ASSETS, CASES, Class};
use crate::oracle::compare::decisions::{Category, Decision, Decisions};
use crate::oracle::compare::metric::{
    Attribution, Calibration, Cause, DivergenceKind, Occurrences, PageState, compare_document,
    controls,
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
        // The tests drive the pipeline with a renderer of their own: `run`, the one path that locates
        // a library and a helper, is reached only where `prepare` refuses.
        outside: None,
        calibration,
        decisions,
        identity,
        pdf_absence: None,
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
        // A key ends in a page, a kind and, for a text or picture divergence, what within it.
        let mut tail = parts.next_back().unwrap_or_default();
        let sub = if matches!(tail, "missing" | "moved" | "extra") {
            let sub = tail;
            tail = parts.next_back().unwrap_or_default();
            Some(sub)
        } else {
            None
        };
        assert!(
            sub.is_none() || matches!(tail, "text" | "picture"),
            "{key}: only a text or picture divergence states what within it"
        );
        let kind = tail;
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
            count: 1,
            cause: Cause::Unexplained,
            category: Category::OutsideQuirk,
            reason: "a decision for this test".to_string(),
        }],
    };
    let (used, stale) = decisions.check(&BTreeMap::new());
    assert!(used.is_empty());
    assert_eq!(stale, ["pdf/text/1/pixels"]);
    let (used, stale) = decisions.check(&BTreeMap::from([(
        "pdf/text/1/pixels".to_string(),
        occurrences(1, Cause::Unexplained),
    )]));
    assert_eq!(used.len(), 1);
    assert!(stale.is_empty());
    // The same key with a different number of divergences is a page the reviewer did not see.
    let (used, stale) = decisions.check(&BTreeMap::from([(
        "pdf/text/1/pixels".to_string(),
        occurrences(2, Cause::Unexplained),
    )]));
    assert!(used.is_empty());
    assert_eq!(stale.len(), 1, "{stale:?}");
    // The same key with the same count and another cause is another finding: the decision accepts
    // a placement difference and the run now shows the words themselves changed.
    let (used, stale) = decisions.check(&BTreeMap::from([(
        "pdf/text/1/pixels".to_string(),
        occurrences(1, Cause::WordsAgree),
    )]));
    assert!(used.is_empty());
    assert_eq!(stale.len(), 1, "{stale:?}");
    assert!(stale[0].contains("words-agree"), "{stale:?}");
}

/// One key's divergences, all of them of one cause.
fn occurrences(count: u32, cause: Cause) -> Occurrences {
    let mut causes = std::collections::BTreeSet::new();
    causes.insert(cause.as_str());
    Occurrences { count, causes }
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
                count: 1,
                cause: Cause::Unexplained,
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

// ── the Office half: the engine's renderer against the committed record ─────────────────────────

/// The Office tolerances the committed calibration states.
fn tolerances() -> crate::oracle::compare::metric::OfficeTolerances {
    calibration().office.clone()
}

/// One corpus document, by id.
fn corpus_document(id: &str) -> &'static crate::oracle::corpus::Document {
    crate::oracle::corpus::documents()
        .iter()
        .find(|document| document.id == id)
        .expect("a corpus document the record covers")
}

/// The committed record entry for one corpus document.
fn entry(id: &str) -> crate::oracle::record::DocumentRecord {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("dev/oracle/record/documents")
        .join(id)
        .join("document.json");
    serde_json::from_str(&std::fs::read_to_string(&path).expect("the committed record entry"))
        .expect("a record entry this tool can read")
}

/// Build and render one corpus document with the engine's own writer and renderer.
fn rendered(id: &str) -> document_engine::render::Rendered {
    let area = TempArea::new("compare-office").unwrap();
    let assets = Assets::write(&area.path().join("assets"), crate::oracle::corpus::ASSETS).unwrap();
    let document = corpus_document(id);
    let built = crate::oracle::engine::build(document, area.path(), &assets).unwrap();
    document_engine::render::render(&built.path, document.name())
}

/// Compare one corpus document with an entry a test may have changed.
fn compare_with(
    id: &str,
    entry: Option<&crate::oracle::record::DocumentRecord>,
) -> crate::oracle::compare::metric::DocumentOutcome {
    crate::oracle::compare::office::compare_entry(
        corpus_document(id),
        entry,
        &rendered(id),
        &tolerances(),
    )
}

/// Every divergence of one kind.
fn kinds(
    outcome: &crate::oracle::compare::metric::DocumentOutcome,
    kind: DivergenceKind,
) -> Vec<&crate::oracle::compare::metric::Divergence> {
    outcome
        .divergences
        .iter()
        .filter(|divergence| divergence.kind == kind)
        .collect()
}

/// The committed entry with the engine's own pages in place of the outside suite's: both sides then
/// state the same thing, so a test of the comparison's mechanism does not depend on the engine
/// matching the outside suite's face metrics.
fn own_entry(id: &str) -> crate::oracle::record::DocumentRecord {
    use crate::oracle::record::{LineRecord, PageRecord, PictureRecord};
    let mut entry = entry(id);
    let document_engine::render::Rendered::Pages { pages, .. } = rendered(id) else {
        panic!("{id} renders");
    };
    entry.pages.pages = pages
        .into_iter()
        .map(|page| PageRecord {
            page: u32::try_from(page.number).expect("a page number"),
            width_pt: page.width_pt,
            height_pt: page.height_pt,
            faces: page.faces,
            lines: page
                .lines
                .into_iter()
                .map(|line| LineRecord {
                    text: line.text,
                    x_pt: line.x_pt,
                    y_pt: line.y_pt,
                    size_pt: line.size_pt,
                })
                .collect(),
            pictures: page
                .pictures
                .into_iter()
                .map(|picture| PictureRecord {
                    x_pt: picture.x_pt,
                    y_pt: picture.y_pt,
                    width_pt: picture.width_pt,
                    height_pt: picture.height_pt,
                })
                .collect(),
        })
        .collect();
    entry
}

#[test]
fn an_office_document_the_record_agrees_with_compares_cleanly() {
    let entry = own_entry("docx/picture");
    let outcome = compare_with("docx/picture", Some(&entry));
    assert_eq!(outcome.document, "docx/picture");
    assert_eq!(outcome.family, Family::Docx);
    assert_eq!(outcome.engine_pages, Some(1));
    assert_eq!(outcome.outside_pages, Some(1));
    assert_eq!(outcome.pages.len(), 1);
    assert_eq!(outcome.pages[0].outcome, PageState::Pass);
    assert!(outcome.divergences.is_empty(), "{:?}", outcome.divergences);
    assert!(outcome.mechanism_failures.is_empty());
    assert!(outcome.not_compared.is_empty());
}

#[test]
fn a_recorded_line_the_record_no_longer_holds_is_a_text_divergence() {
    let mut entry = entry("docx/picture");
    entry.pages.pages[0]
        .lines
        .retain(|line| line.text != "Below the picture.");
    let outcome = compare_with("docx/picture", Some(&entry));
    let texts = kinds(&outcome, DivergenceKind::Text);
    assert_eq!(texts.len(), 1, "{:?}", outcome.divergences);
    assert!(
        texts[0].detail.contains("Below the picture."),
        "{}",
        texts[0].detail
    );
    assert_eq!(texts[0].page, Some(1));
    assert_eq!(texts[0].key, "docx/picture/1/text/extra");
    assert_eq!(outcome.pages[0].outcome, PageState::Divergence);
}

#[test]
fn a_recorded_line_that_moved_beyond_the_tolerance_is_a_text_divergence() {
    let mut entry = entry("docx/picture");
    let line = entry.pages.pages[0]
        .lines
        .iter_mut()
        .find(|line| line.text == "Below the picture.")
        .expect("the record holds the line");
    line.y_pt += 40.0;
    let outcome = compare_with("docx/picture", Some(&entry));
    let texts = kinds(&outcome, DivergenceKind::Text);
    assert_eq!(texts.len(), 1, "{:?}", outcome.divergences);
    assert!(
        texts[0].detail.contains("Below the picture."),
        "{}",
        texts[0].detail
    );
    assert!(
        texts[0].detail.contains("the engine draws it at"),
        "a moved line says where each side put it: {}",
        texts[0].detail
    );
}

#[test]
fn a_recorded_picture_the_record_no_longer_holds_is_a_picture_divergence() {
    let mut entry = entry("docx/picture");
    entry.pages.pages[0].pictures.clear();
    let outcome = compare_with("docx/picture", Some(&entry));
    let pictures = kinds(&outcome, DivergenceKind::Picture);
    assert_eq!(pictures.len(), 1, "{:?}", outcome.divergences);
    assert!(
        pictures[0].detail.contains("the engine draws a picture"),
        "{}",
        pictures[0].detail
    );
    assert_eq!(pictures[0].key, "docx/picture/1/picture/extra");
}

#[test]
fn a_recorded_page_size_beyond_the_tolerance_is_a_page_size_divergence() {
    let mut entry = entry("docx/picture");
    entry.pages.pages[0].width_pt += 10.0;
    let outcome = compare_with("docx/picture", Some(&entry));
    let sizes = kinds(&outcome, DivergenceKind::PageSize);
    assert_eq!(sizes.len(), 1, "{:?}", outcome.divergences);
    assert!(sizes[0].detail.contains("605.304"), "{}", sizes[0].detail);
    assert!(sizes[0].detail.contains("595.300"), "{}", sizes[0].detail);
    assert_eq!(sizes[0].key, "docx/picture/1/page-size");
}

#[test]
fn a_recorded_page_size_within_the_tolerance_is_not_a_divergence() {
    let mut entry = own_entry("docx/picture");
    entry.pages.pages[0].width_pt += tolerances().page_size_pt / 2.0;
    let outcome = compare_with("docx/picture", Some(&entry));
    assert!(outcome.divergences.is_empty(), "{:?}", outcome.divergences);
}

#[test]
fn a_corpus_document_the_record_does_not_cover_is_not_compared() {
    let outcome = compare_with("docx/picture", None);
    assert!(outcome.divergences.is_empty(), "{:?}", outcome.divergences);
    assert_eq!(outcome.not_compared.len(), 1);
    assert!(outcome.not_compared[0].page.is_none());
    assert!(
        outcome.not_compared[0]
            .reason
            .contains("no entry for docx/picture"),
        "{}",
        outcome.not_compared[0].reason
    );
    assert_eq!(outcome.engine_pages, Some(1));
}

#[test]
fn an_engine_that_refuses_a_recorded_document_fails_the_mechanism() {
    let entry = entry("docx/picture");
    let refused = document_engine::render::Rendered::Refused(document_engine::operation::Failure {
        class: "internal".to_string(),
        reason: "the engine says no".to_string(),
    });
    let outcome = crate::oracle::compare::office::compare_entry(
        corpus_document("docx/picture"),
        Some(&entry),
        &refused,
        &tolerances(),
    );
    assert_eq!(outcome.mechanism_failures.len(), 1);
    assert_eq!(
        outcome.mechanism_failures[0].kind,
        crate::oracle::compare::metric::MechanismKind::EngineRefused
    );
    assert!(
        outcome.mechanism_failures[0]
            .detail
            .contains("docx/picture")
    );
}

#[test]
fn the_coverage_table_names_documents_the_corpus_declares_and_reasons_where_it_cannot() {
    let known: BTreeSet<&str> = crate::oracle::corpus::documents()
        .iter()
        .map(|document| document.id)
        .collect();
    let rows = crate::oracle::compare::office::coverage().unwrap();
    assert_eq!(rows.len(), crate::oracle::compare::office::COVERAGE.len());
    for row in &rows {
        match (&row.document, &row.reason) {
            (Some(document), None) => assert!(known.contains(document), "{}", row.feature),
            (None, Some(reason)) => assert!(!reason.trim().is_empty(), "{}", row.feature),
            (None, None) => panic!("{} states neither a document nor a reason", row.feature),
            (Some(_), Some(_)) => panic!("{} states both a document and a reason", row.feature),
        }
    }
    // The legacy formats are a named exception, not a silent omission.
    let legacy = rows
        .iter()
        .find(|row| row.feature.starts_with("legacy"))
        .expect("the legacy formats are in the table");
    assert!(legacy.document.is_none());
    assert!(
        legacy
            .reason
            .is_some_and(|reason| reason.contains("separate body of work")),
        "{:?}",
        legacy.reason
    );
}

#[test]
fn the_coverage_table_refuses_a_row_that_states_a_document_and_a_reason() {
    let refused = crate::oracle::compare::office::checked(&[(
        "a feature stated twice",
        Some("docx/report"),
        Some("a reason for the same feature"),
    )]);
    let error = refused.expect_err("a row stating both is refused");
    assert!(error.contains("a feature stated twice"), "{error}");
}

#[test]
fn the_report_names_the_office_quantities_and_no_longer_denies_a_renderer() {
    let entry = entry("docx/picture");
    let outcome = compare_with("docx/picture", Some(&entry));
    let report = crate::oracle::compare::report::Report::new(
        prepared(Decisions {
            schema: 1,
            decisions: Vec::new(),
        })
        .identity,
        crate::oracle::compare::report::Scope::new(&[], &[], "").with_office(
            crate::oracle::compare::OFFICE_SCOPE,
            crate::oracle::compare::OFFICE_NOT_COMPARED,
            crate::oracle::compare::OFFICE_DOES_NOT_PROVE,
        ),
        Vec::new(),
        vec![outcome],
        Vec::new(),
        Vec::new(),
    );
    let mut out = Vec::new();
    crate::oracle::compare::report::print(&report, &mut out);
    let text = String::from_utf8(out).unwrap();
    assert!(
        text.contains(
            "office compared: page count, page size, line text, position and size, pictures \
             drawn, faces used"
        ),
        "{text}"
    );
    assert!(
        text.contains("office not compared: face identity, glyph shapes, pixel appearance"),
        "{text}"
    );
    assert!(text.contains("coverage:"), "{text}");
    assert!(text.contains("Word headings: docx/report"), "{text}");
    assert!(!text.contains("no page renderer"), "{text}");
    assert!(!text.contains("has no page renderer"), "{text}");
}

#[test]
fn a_family_the_record_covers_with_no_outcome_says_so_about_the_record() {
    let report = crate::oracle::compare::report::Report::new(
        prepared(Decisions {
            schema: 1,
            decisions: Vec::new(),
        })
        .identity,
        crate::oracle::compare::report::Scope::new(&[], &[], ""),
        Vec::new(),
        Vec::new(),
        Vec::new(),
        Vec::new(),
    );
    let mut out = Vec::new();
    crate::oracle::compare::report::print(&report, &mut out);
    let text = String::from_utf8(out).unwrap();
    assert!(
        text.contains(
            "docx: not compared — the record covers no Word document, so there is no outside \
             opinion to compare the engine's pages against"
        ),
        "{text}"
    );
    assert!(!text.contains("no page renderer"), "{text}");
}

#[test]
fn the_office_half_runs_without_the_rasteriser_and_the_pdf_half_says_why() {
    let area = TempArea::new("compare-no-rasteriser").unwrap();
    let mut prepared = prepared(Decisions {
        schema: 1,
        decisions: Vec::new(),
    });
    prepared.pdf_absence = Some("there is no PDFium on this machine".to_string());
    let mut options = Options::new();
    options.evidence = area.path().join("evidence");
    let mut out = Vec::new();
    let summary = match compare(&options, &prepared, &mut out, |_, _, _| {
        panic!("the PDF half must not render a page without a rasteriser")
    }) {
        Ok(Ran::Compared(summary)) => summary,
        other => {
            panic!("a run without a rasteriser must still compare the office pages: {other:?}")
        }
    };
    let text = String::from_utf8(out).unwrap();
    assert!(
        text.contains("pdf: not compared — there is no PDFium on this machine"),
        "{text}"
    );
    assert!(text.contains("office compared: page count"), "{text}");
    assert!(summary.compared > 0, "{summary:?}");
    assert!(!summary.failed, "{summary:?}");
    assert_eq!(summary.pixel_pages, 0);
}

#[test]
fn a_record_grouped_by_another_line_band_is_refused_before_a_page_is_judged() {
    // The record's lines were grouped by the band the identity states. Judging with another band
    // would report the grouping difference as a drawing defect, so the Office half refuses.
    let area = TempArea::new("compare-ambient").unwrap();
    let root = area.path().join("record");
    std::fs::create_dir_all(&root).unwrap();
    let committed = crate::oracle::run::record_root();
    std::fs::copy(committed.join("fonts.conf"), root.join("fonts.conf")).unwrap();
    let identity: serde_json::Value =
        serde_json::from_slice(&std::fs::read(committed.join("identity.json")).unwrap()).unwrap();
    let mut identity = identity;
    identity["line_band_pt"] = serde_json::json!(identity["line_band_pt"].as_f64().unwrap() + 1.0);
    std::fs::write(
        root.join("identity.json"),
        serde_json::to_vec_pretty(&identity).unwrap(),
    )
    .unwrap();
    match crate::oracle::compare::office::run(&tolerances(), &root) {
        Err(Error::Refused(reason)) => assert!(
            reason.contains("the line band is"),
            "the refusal names the band: {reason}"
        ),
        other => panic!("a record grouped by another band must be refused: {other:?}"),
    }
}

#[test]
fn a_recorded_page_the_engine_does_not_hold_is_a_page_count_divergence() {
    let mut entry = entry("docx/picture");
    let mut extra = entry.pages.pages[0].clone();
    extra.page = 2;
    extra.lines.clear();
    extra.pictures.clear();
    extra.faces.clear();
    entry.pages.pages.push(extra);
    let outcome = compare_with("docx/picture", Some(&entry));
    let counts = kinds(&outcome, DivergenceKind::PageCount);
    assert_eq!(counts.len(), 1, "{:?}", outcome.divergences);
    assert!(counts[0].page.is_none());
    assert_eq!(counts[0].key, "docx/picture/document/page-count");
    assert!(
        counts[0].detail.contains("the engine holds 1 page"),
        "{}",
        counts[0].detail
    );
    assert_eq!(outcome.not_compared.len(), 1);
    assert_eq!(outcome.not_compared[0].page, Some(2));
    assert!(
        outcome.not_compared[0]
            .reason
            .contains("the engine holds no such page"),
        "{}",
        outcome.not_compared[0].reason
    );
}

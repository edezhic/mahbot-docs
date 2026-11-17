//! Supervision: a case that exhausts memory, writes without limit or hangs costs that case, and
//! the verdicts already decided survive.
//!
//! The helper binary (`CARGO_BIN_EXE_parity-helper`) speaks the child protocol and selects its
//! behaviour per case from the environment the supervisor hands the child, so a partial run is
//! produced deterministically and no test touches its own process environment.
//!
//! Supervision requires process groups and a descendant walk, which macOS and Linux provide; where
//! they are unavailable the gate refuses to run, so these tests run only where it does. The memory
//! bound is macOS-only, so its tests are gated there too.
#![cfg(any(target_os = "macos", target_os = "linux"))]
mod common;

use document_parity::baseline::Baseline;
use document_parity::engine::{Reader, Writer};
use document_parity::harness::{CutShort, Harness, Report, Supervised};
use document_parity::reference::HelperProbe;
use document_parity::supervisor::{Bounds, EngineKind, Supervision};
use std::io::{BufRead as _, BufReader, Read as _};
use std::path::PathBuf;
use std::time::{Duration, Instant};

const HELPER: &str = env!("CARGO_BIN_EXE_parity-helper");
const PARITY: &str = env!("CARGO_BIN_EXE_parity");

/// The slug of the first case whose fixture is far under any lowered file bound.
fn small_case() -> String {
    small_cases(1).pop().expect("a small case")
}

fn small_cases(count: usize) -> Vec<String> {
    let mut small = Vec::new();
    for case in document_parity::corpus::writer_cases() {
        let size: usize = case
            .documents
            .iter()
            .map(|document| (document.build)().len())
            .sum();
        if size < 256 * 1024 {
            small.push(case.key().slug());
            if small.len() == count {
                return small;
            }
        }
    }
    panic!("the corpus holds fewer than {count} cases whose fixtures are under 256 KiB");
}

/// Run the helper over the whole corpus with the documented bounds, one per-case override and one
/// helper behaviour. The suite's concurrency permit is held for the whole run, so the cap covers the
/// corpus's children.
fn helper_run(
    override_for: Option<(String, Bounds)>,
    env: &[(&str, &str)],
    collect: bool,
) -> Supervised {
    let _permit = common::permit();
    let harness = Harness::new_with(|_| {
        Ok((
            Box::new(HelperProbe) as Box<dyn Writer>,
            Box::new(HelperProbe) as Box<dyn Reader>,
        ))
    })
    .expect("a harness root");
    let baseline = Baseline::load(&Baseline::default_path()).expect("a baseline");
    let mut supervision = Supervision::new(EngineKind::Helper, PathBuf::from(HELPER));
    supervision.override_for = override_for;
    supervision.collect = collect;
    supervision.env = env
        .iter()
        .map(|(name, value)| ((*name).to_string(), (*value).to_string()))
        .collect();
    harness
        .supervise(&baseline, &mut common::NullSink, &supervision)
        .expect("a supervised run")
}

/// The problems the verdict for `slug` carries.
fn problem(report: &Report, slug: &str) -> String {
    report
        .verdicts
        .iter()
        .find(|verdict| verdict.key.slug() == slug)
        .unwrap_or_else(|| panic!("the run has no verdict for {slug}"))
        .problems
        .join("; ")
}

/// The problems the verdict for the reading `name` carries.
fn reading_problem(report: &Report, name: &str) -> String {
    report
        .readings
        .iter()
        .find(|reading| reading.document == name)
        .unwrap_or_else(|| panic!("the run has no verdict for the reading {name}"))
        .problems
        .join("; ")
}

/// The cases whose verdict names a resource bound, so a test that lowers one bound can show it
/// affected only its target and no legitimate case. The phrases are the four the supervisor's own
/// bound sentences use, not the bare word: a case slug or a request can hold "boundary".
fn bound_failures(report: &Report) -> Vec<String> {
    const BOUNDS: [&str; 4] = [
        "per-file bound",
        "directory bound",
        "entry bound",
        "memory bound",
    ];
    let mut slugs: Vec<String> = BOUNDS
        .iter()
        .flat_map(|bound| failures_naming(report, bound))
        .collect();
    slugs.sort();
    slugs.dedup();
    slugs
}

/// The cases whose verdict carries a problem naming `phrase`.
fn failures_naming(report: &Report, phrase: &str) -> Vec<String> {
    report
        .verdicts
        .iter()
        .filter(|verdict| {
            verdict
                .problems
                .iter()
                .any(|problem| problem.contains(phrase))
        })
        .map(|verdict| verdict.key.slug())
        .collect()
}

#[test]
fn the_documented_bounds_are_the_defaults() {
    // The values the gate actually uses, not a second copy of them: the per-case bounds come from
    // `Bounds::default()`, and the run budget from `RUN_BUDGET`, which is outside `Bounds`. The
    // value must be the row's Default cell, not merely somewhere in the row: a number stated for
    // the wrong bound would otherwise pass.
    let bounds = Bounds::default();
    let readme = include_str!("../README.md");
    let row = |name: &str| {
        readme
            .lines()
            .filter(|line| line.starts_with('|'))
            .find(|row| {
                row.split('|')
                    .nth(1)
                    .is_some_and(|cell| cell.trim().contains(name))
            })
            .unwrap_or_else(|| panic!("the README should have a row naming `{name}`"))
            .split('|')
            .nth(2)
            .expect("the README's row has a Default cell")
            .trim()
            .to_string()
    };
    for (name, value) in [
        (
            "physical footprint",
            format!("{} MiB", bounds.memory_bytes / (1024 * 1024)),
        ),
        (
            "one file's apparent size",
            format!("{} MiB", bounds.file_bytes / (1024 * 1024)),
        ),
        (
            "a case's directory, allocated bytes",
            format!("{} GiB", bounds.dir_bytes / (1024 * 1024 * 1024)),
        ),
        (
            "entries counted recursively",
            format!("{} entries", bounds.dir_entries),
        ),
        (
            "read of one transcript",
            format!("{} MiB", bounds.transcript_bytes / (1024 * 1024)),
        ),
        (
            "poller's interval",
            format!("{} ms", bounds.poll.as_millis()),
        ),
        (
            "grace before a dead owner's root",
            format!("{} s", bounds.grace.as_secs()),
        ),
        (
            "one writer case's supervisor budget",
            format!("{} s", bounds.writer_timeout.as_secs()),
        ),
        (
            "one reading's supervisor budget",
            format!("{} s", bounds.reading_timeout.as_secs()),
        ),
        (
            "the whole run's budget",
            format!("{} s", document_parity::harness::RUN_BUDGET.as_secs()),
        ),
    ] {
        let stated = row(name);
        assert_eq!(
            stated, value,
            "the README's `{name}` row should state `{value}` as its default"
        );
    }
}

#[test]
fn every_helper_behaviour_is_stated_in_the_readme_and_every_row_has_one() {
    // The README is the one home of the helper's behaviours: a behaviour added to the helper and
    // not stated there would be undocumented, and a row left behind for a removed behaviour would
    // describe something the helper no longer does.
    let source = include_str!("../src/bin/parity-helper.rs");
    let readme = include_str!("../README.md");
    let mut names: Vec<&str> = Vec::new();
    let mut rest = source;
    while let Some(start) = rest.find("\"PARITY_HELPER_") {
        rest = &rest[start + 1..];
        let end = rest.find('"').expect("a closing quote");
        let name = &rest[..end];
        // A path variable, not a behaviour: it names where the hanging case writes its pid.
        if name != "PARITY_HELPER_PIDFILE" && !names.contains(&name) {
            names.push(name);
        }
    }
    assert!(!names.is_empty(), "the helper names its behaviours");
    for name in &names {
        assert!(readme.contains(name), "the README should state `{name}`");
    }
    for line in readme.lines().filter(|line| line.starts_with('|')) {
        let Some(name) = line.split('|').nth(1) else {
            continue;
        };
        let name = name.trim();
        if name.starts_with("`PARITY_HELPER_") {
            let name = name.trim_matches('`');
            assert!(
                names.contains(&name),
                "the README states `{name}`, which the helper no longer implements"
            );
        }
    }
}

#[cfg(target_os = "macos")]
#[test]
fn a_case_that_allocates_without_limit_is_one_failed_verdict() {
    let slug = small_case();
    let bounds = Bounds {
        memory_bytes: 512 * 1024 * 1024,
        ..Bounds::default()
    };
    let supervised = helper_run(
        Some((slug.clone(), bounds)),
        &[("PARITY_HELPER_RUNAWAY", &slug)],
        false,
    );
    let report = &supervised.report;
    assert!(supervised.cut_short.is_none(), "the run was not cut short");
    assert_eq!(
        report.verdicts.len(),
        document_parity::corpus::writer_cases().len(),
        "every case still produces a verdict"
    );
    assert!(
        problem(report, &slug).contains("memory bound"),
        "the runaway's verdict names the memory bound: {}",
        problem(report, &slug)
    );
    // The runaway is the only case that should be failed by the memory bound; a legitimate case
    // that happened to be killed by the lowered ceiling would be named here.
    assert_eq!(failures_naming(report, "memory bound"), vec![slug]);
}

#[cfg(target_os = "macos")]
#[test]
fn a_reading_that_allocates_without_limit_is_one_failed_verdict() {
    let documents = document_parity::corpus::reader_documents();
    let name = documents[0].name;
    let bounds = Bounds {
        memory_bytes: 512 * 1024 * 1024,
        ..Bounds::default()
    };
    let supervised = helper_run(
        Some((name.to_string(), bounds)),
        &[("PARITY_HELPER_READING_RUNAWAY", name)],
        false,
    );
    let report = &supervised.report;
    assert!(supervised.cut_short.is_none(), "the run was not cut short");
    assert_eq!(
        report.readings.len(),
        documents.len(),
        "every reading still produces a verdict"
    );
    assert!(
        reading_problem(report, name).contains("memory bound"),
        "the runaway reading's verdict names the memory bound: {}",
        reading_problem(report, name)
    );
}

#[cfg(target_os = "macos")]
#[test]
fn a_case_that_finishes_above_the_ceiling_is_failed() {
    // The poller is set to an interval far longer than this case, so it never samples the spike;
    // the only source of the peak is the exact read before the child is reaped. If that read were
    // removed, the verdict would be a pass and this test would fail.
    let slug = small_case();
    let bounds = Bounds {
        memory_bytes: 256 * 1024 * 1024,
        poll: Duration::from_secs(10),
        ..Bounds::default()
    };
    let supervised = helper_run(
        Some((slug.clone(), bounds)),
        &[("PARITY_HELPER_SPIKE", &slug)],
        false,
    );
    let report = &supervised.report;
    assert!(supervised.cut_short.is_none(), "the run was not cut short");
    assert!(
        problem(report, &slug).contains("memory bound"),
        "a case that finished above the ceiling is failed: {}",
        problem(report, &slug)
    );
}

#[test]
fn the_file_size_bound_names_itself() {
    let slug = small_case();
    let bounds = Bounds {
        file_bytes: 8 * 1024 * 1024,
        ..Bounds::default()
    };
    let supervised = helper_run(
        Some((slug.clone(), bounds)),
        &[("PARITY_HELPER_BIG_FILE", &slug)],
        false,
    );
    let report = &supervised.report;
    assert!(
        problem(report, &slug).contains("per-file bound"),
        "the runaway writer's verdict names the per-file bound: {}",
        problem(report, &slug)
    );
    assert_eq!(
        report.verdicts.len(),
        document_parity::corpus::writer_cases().len(),
        "every case still produces a verdict"
    );
    assert_eq!(
        bound_failures(report),
        vec![slug],
        "only the target hit a bound; the lowered limit did not fail a legitimate case"
    );
}

#[test]
fn the_directory_size_bound_names_itself() {
    // The flood keeps writing past the first poll interval, and the entry bound is left at its
    // default so only the directory-size bound can explain the kill.
    let slug = small_case();
    let bounds = Bounds {
        dir_bytes: 8 * 1024 * 1024,
        ..Bounds::default()
    };
    let supervised = helper_run(
        Some((slug.clone(), bounds)),
        &[("PARITY_HELPER_FLOOD_FILES", &slug)],
        false,
    );
    let report = &supervised.report;
    let problem = problem(report, &slug);
    assert!(
        problem.contains("directory bound"),
        "the runaway writer's verdict names the directory bound: {problem}"
    );
    assert!(
        !problem.contains("entry bound"),
        "the directory-size bound alone is what the verdict names: {problem}"
    );
    assert_eq!(
        report.verdicts.len(),
        document_parity::corpus::writer_cases().len(),
        "every case still produces a verdict"
    );
    assert_eq!(
        bound_failures(report),
        vec![slug],
        "only the target hit a bound; the lowered limit did not fail a legitimate case"
    );
}

#[test]
fn the_directory_bound_outranks_the_entry_bound() {
    // Both bounds are tripped: the flood writes fast enough that the directory holds more than
    // both bounds at the first sample, and the stated order names the directory-size bound.
    let slug = small_case();
    let bounds = Bounds {
        dir_bytes: 2 * 1024 * 1024,
        dir_entries: 24,
        ..Bounds::default()
    };
    let supervised = helper_run(
        Some((slug.clone(), bounds)),
        &[("PARITY_HELPER_FLOOD_FILES", &slug)],
        false,
    );
    let report = &supervised.report;
    let problem = problem(report, &slug);
    assert!(
        problem.contains("directory bound"),
        "the directory bound outranks the entry bound: {problem}"
    );
    assert!(
        !problem.contains("entry bound"),
        "the verdict names only the first bound in the stated order: {problem}"
    );
    assert_eq!(
        report.verdicts.len(),
        document_parity::corpus::writer_cases().len(),
        "every case still produces a verdict"
    );
    assert_eq!(
        bound_failures(report),
        vec![slug],
        "only the target hit a bound; the lowered limit did not fail a legitimate case"
    );
}

#[test]
fn the_entry_count_bound_names_itself() {
    let slug = small_case();
    let bounds = Bounds {
        dir_entries: 32,
        ..Bounds::default()
    };
    let supervised = helper_run(
        Some((slug.clone(), bounds)),
        &[("PARITY_HELPER_FLOOD_ENTRIES", &slug)],
        false,
    );
    let report = &supervised.report;
    assert!(
        problem(report, &slug).contains("entry bound"),
        "the runaway writer's verdict names the entry bound: {}",
        problem(report, &slug)
    );
    assert_eq!(
        report.verdicts.len(),
        document_parity::corpus::writer_cases().len(),
        "every case still produces a verdict"
    );
    assert_eq!(
        bound_failures(report),
        vec![slug],
        "only the target hit a bound; the lowered limit did not fail a legitimate case"
    );
}

#[test]
fn a_case_that_outlives_the_supervisor_budget_is_one_failed_verdict() {
    // The hanging helper sleeps inside its own run, so no disk or memory bound explains the kill:
    // the supervisor's own budget is what the verdict names.
    let slug = small_case();
    let bounds = Bounds {
        writer_timeout: Duration::from_secs(1),
        ..Bounds::default()
    };
    let supervised = helper_run(
        Some((slug.clone(), bounds)),
        &[("PARITY_HELPER_HANG", &slug)],
        false,
    );
    let report = &supervised.report;
    assert!(supervised.cut_short.is_none(), "the run was not cut short");
    let problem = problem(report, &slug);
    assert!(
        problem.contains("did not finish within the supervisor's 1s budget"),
        "the case that outlived its budget is named by the writer's own 1s budget: {problem}"
    );
    assert!(
        !problem.contains("bound"),
        "a timeout is named only when no bound explains the case: {problem}"
    );
    assert_eq!(
        report.verdicts.len(),
        document_parity::corpus::writer_cases().len(),
        "every case still produces a verdict"
    );
}

#[test]
fn a_reading_that_outlives_the_supervisor_budget_is_one_failed_verdict() {
    // The hanging reader sleeps inside its own call, so no disk or memory bound explains the kill:
    // the reading's own supervisor budget is what the verdict names. This is the branch that
    // `timeout_for`'s `ChildTarget::Reading` arm chooses.
    let documents = document_parity::corpus::reader_documents();
    let name = documents[0].name;
    let bounds = Bounds {
        reading_timeout: Duration::from_secs(1),
        ..Bounds::default()
    };
    let supervised = helper_run(
        Some((name.to_string(), bounds)),
        &[("PARITY_HELPER_READING_HANG", name)],
        false,
    );
    let report = &supervised.report;
    assert!(supervised.cut_short.is_none(), "the run was not cut short");
    let problem = reading_problem(report, name);
    assert!(
        problem.contains("did not finish within the supervisor's 1s budget"),
        "the reading that outlived its budget is named by the reading's own 1s budget: {problem}"
    );
    assert!(
        !problem.contains("bound"),
        "a timeout is named only when no bound explains the reading: {problem}"
    );
    assert_eq!(
        report.readings.len(),
        documents.len(),
        "every reading still produces a verdict"
    );
}

#[test]
fn a_transcript_over_the_read_cap_is_one_failed_verdict() {
    let slug = small_case();
    let bounds = Bounds {
        transcript_bytes: 4 * 1024,
        ..Bounds::default()
    };
    let supervised = helper_run(
        Some((slug.clone(), bounds)),
        &[("PARITY_HELPER_BIG_TRANSCRIPT", &slug)],
        false,
    );
    let report = &supervised.report;
    assert!(
        problem(report, &slug).contains("read limit"),
        "the oversized transcript is one failed verdict naming the cap: {}",
        problem(report, &slug)
    );
    assert_eq!(
        report.verdicts.len(),
        document_parity::corpus::writer_cases().len(),
        "every case still produces a verdict"
    );
}

#[test]
fn an_unparseable_transcript_is_one_failed_verdict() {
    // The helper writes a real transcript and then makes it unparseable, so the parent's "does not
    // parse" branch is one failed verdict rather than a crash of the run.
    let slug = small_case();
    let supervised = helper_run(None, &[("PARITY_HELPER_BAD_TRANSCRIPT", &slug)], false);
    let report = &supervised.report;
    assert!(supervised.cut_short.is_none(), "the run was not cut short");
    assert!(
        problem(report, &slug).contains("does not parse"),
        "an unparseable transcript is named: {}",
        problem(report, &slug)
    );
    assert_eq!(
        report.verdicts.len(),
        document_parity::corpus::writer_cases().len(),
        "every other case still produces a verdict"
    );
}

#[test]
fn an_in_process_and_a_supervised_transcript_agree() {
    let slug = small_case();
    // The in-process path: a fake engine whose failure quotes the scratch path it was given.
    struct Failing;
    impl Writer for Failing {
        fn identity(&self) -> document_parity::engine::EngineIdentity {
            document_parity::engine::EngineIdentity {
                engine: "failing".to_string(),
                build: "failing".to_string(),
                runtime: None,
                components: std::collections::BTreeMap::new(),
            }
        }
        fn run(
            &self,
            _request: &serde_json::Value,
            scratch: &std::path::Path,
        ) -> anyhow::Result<document_parity::engine::WriterOutcome> {
            anyhow::bail!("the engine fell over in {}", scratch.display())
        }
    }
    let harness = Harness::at(
        Harness::temp_root(),
        Box::new(Failing) as Box<dyn Writer>,
        Box::new(HelperProbe) as Box<dyn Reader>,
    )
    .expect("a harness root");
    let in_process = harness
        .run()
        .outcomes
        .into_iter()
        .find(|outcome| outcome.key.slug() == slug)
        .expect("the case ran")
        .problem
        .expect("the case failed");
    // The supervised path: the helper fails with the same words. The normaliser is root-relative,
    // so both transcripts carry `<run>` where the harness's root was.
    let run = helper_run(None, &[("PARITY_HELPER_FAILING", &slug)], true).run;
    let under_supervision = run
        .expect("collected")
        .outcomes
        .into_iter()
        .find(|outcome| outcome.key.slug() == slug)
        .expect("the case ran")
        .problem
        .expect("the case failed");
    assert_eq!(
        in_process, under_supervision,
        "the same failure normalises the same way in process and under supervision"
    );
    assert!(
        under_supervision.contains("<run>"),
        "the child's root is replaced, not dropped: {under_supervision}"
    );
}

#[test]
fn the_supervisor_budgets_exceed_the_reference_writers_own_bounds() {
    // The reference writer's bounds are private and live in a hashed module that cannot be edited.
    // The supervisor's budget must sit above them, or a hung runtime becomes a killed case instead
    // of a transcript; the numbers are restated here so a change in the hashed module is noticed.
    let source = include_str!("../src/reference/writer.rs");
    assert!(
        source.contains("const RUN_TIMEOUT: Duration = Duration::from_secs(120);"),
        "the reference writer's run timeout moved"
    );
    assert!(
        source.contains("const VERSION_TIMEOUT: Duration = Duration::from_secs(30);"),
        "the reference writer's version-probe timeout moved"
    );
    let bounds = Bounds::default();
    assert!(bounds.writer_timeout > Duration::from_secs(120 + 30));
    assert!(bounds.reading_timeout > Duration::from_secs(120 + 120));
}

#[test]
fn a_child_that_dies_outside_any_bound_is_one_failed_verdict() {
    let cases = document_parity::corpus::writer_cases();
    let small = small_cases(2);
    let supervised = helper_run(
        None,
        &[
            ("PARITY_HELPER_ABORT", &small[0]),
            ("PARITY_HELPER_EXIT", &small[1]),
        ],
        false,
    );
    let report = &supervised.report;
    assert!(supervised.cut_short.is_none(), "the run was not cut short");
    assert_eq!(
        report.verdicts.len(),
        cases.len(),
        "every case still produces a verdict"
    );
    assert!(
        problem(report, &small[0]).contains("killed by signal"),
        "a child killed by a signal is named: {}",
        problem(report, &small[0])
    );
    assert!(
        problem(report, &small[1]).contains("exited with status 7 without a transcript"),
        "a child that exited without a transcript is named: {}",
        problem(report, &small[1])
    );
}

#[test]
fn a_leftover_root_is_reclaimed_only_when_its_owner_is_dead_and_the_grace_passed() {
    // A scratch directory of this test's own, so the sweep considers these roots and no other
    // run's: a dead root an earlier run left in the shared temporary directory cannot change what
    // the sweep here is expected to reclaim.
    let scratch = tempfile::tempdir_in(std::env::temp_dir()).expect("a scratch directory");

    // The sweep's own root, marked dead: the `own_root` guard, not the liveness rule, is what has
    // to keep it.
    let parent = Temp(scratch.path().join("document-parity-parent"));
    mark_dead(&parent.0);

    // A second marked root whose owner is alive. It is not the sweep's own root, so the liveness
    // rule is what has to leave it alone.
    let live = Temp(scratch.path().join("document-parity-live"));
    std::fs::create_dir_all(&live.0).expect("a live root");
    // A marker with no claim held: the claim is taken and released at once, which is the one way a
    // marker is written.
    drop(document_parity::supervisor::Claim::take(&live.0).expect("a live marker"));

    // A marked root that records a start time which does not match its live identifier: the owner
    // it names is gone, the same evidence a reused identifier leaves. This does not rely on a
    // dead pid or on the platform's errno for one.
    let dead = Temp(scratch.path().join("document-parity-dead"));
    mark_dead(&dead.0);

    let bounds = Bounds {
        grace: Duration::from_secs(3600),
        ..Bounds::default()
    };
    let reclaimed = document_parity::supervisor::sweep_in(scratch.path(), Some(&parent.0), &bounds);
    assert!(
        dead.0.exists(),
        "a dead owner's root is not reclaimed before the grace"
    );
    assert!(
        reclaimed.is_empty(),
        "a sweep reports nothing it left alone"
    );
    assert!(live.0.exists(), "a live owner's root is left alone");

    // The roots were created before the first sweep, so they are older than a zero grace; the
    // sleep covers a coarse filesystem clock, not a zero age.
    std::thread::sleep(Duration::from_millis(20));
    let bounds = Bounds {
        grace: Duration::ZERO,
        ..Bounds::default()
    };
    let reclaimed = document_parity::supervisor::sweep_in(scratch.path(), Some(&parent.0), &bounds);
    assert!(
        live.0.exists(),
        "a live owner's root is left alone even after the grace"
    );
    assert!(
        !dead.0.exists(),
        "a dead owner's root is reclaimed once the grace passes"
    );
    assert_eq!(
        reclaimed,
        vec![dead.0.clone()],
        "a sweep reports every root it reclaimed, and nothing it left alone"
    );
    assert!(parent.0.exists(), "the sweep never removes its own root");
}

#[test]
fn a_root_whose_claim_is_held_survives_a_sweep_that_reads_its_owner_as_dead() {
    // The liveness probe can be wrong — a reused identifier, another pid namespace, a filesystem
    // clock — so the guarantee must not rest on it. Every root the harness creates is claimed, and
    // the claim outranks the probe. Going through `Harness::at` is the point: a harness that stopped
    // claiming its root would leave this root to the probe, and this test would fail.
    // A scratch directory under the OS temporary area, so the sweep below considers this test's
    // root and no other test's: sweeping the shared temporary directory would race the tests that
    // keep their own dead roots there.
    let scratch = tempfile::tempdir_in(std::env::temp_dir()).expect("a scratch directory");
    let root = scratch.path().join("document-parity-claimed");
    let harness = Harness::at(
        root.clone(),
        Box::new(document_parity::reference::NoOpWriter),
        Box::new(document_parity::reference::NoOpReader),
    )
    .expect("a harness root");
    // The probe now reads the marker's owner as gone: the identifier is live, its recorded start
    // time is not. Before the claim, that was the whole decision.
    mark_dead(&root);

    let bounds = Bounds {
        grace: Duration::ZERO,
        ..Bounds::default()
    };
    std::thread::sleep(Duration::from_millis(20));
    let reclaimed = document_parity::supervisor::sweep_in(scratch.path(), None, &bounds);
    assert!(
        reclaimed.is_empty(),
        "a sweep reports nothing it left alone"
    );
    assert!(
        root.exists(),
        "a root a live harness created is left alone, whatever the probe says"
    );
    // The harness removes its own root, so nothing is left for a later sweep to collect.
    drop(harness);
    assert!(!root.exists(), "the harness removes its own root");
}

#[test]
fn an_abandoned_root_is_reclaimed_once_nobody_claims_it() {
    let scratch = tempfile::tempdir().expect("a scratch directory");
    let abandoned = scratch.path().join("document-parity-abandoned");
    std::fs::create_dir_all(&abandoned).expect("an abandoned root");
    let claim = document_parity::supervisor::Claim::take(&abandoned).expect("a claim");
    mark_dead(&abandoned);
    // The owner is gone in truth: the lock is released with the guard.
    drop(claim);
    std::thread::sleep(Duration::from_millis(20));
    let bounds = Bounds {
        grace: Duration::ZERO,
        ..Bounds::default()
    };
    let reclaimed = document_parity::supervisor::sweep_in(scratch.path(), None, &bounds);
    assert_eq!(
        reclaimed,
        vec![abandoned.clone()],
        "an abandoned root is reclaimed once nobody claims it"
    );
}

#[test]
fn a_sweep_never_takes_a_runs_transcripts() {
    // A transcript is what a verdict is read from, so it lives beside the run's root and is not
    // named like one: a sweep working in that same directory reclaims the dead root and leaves the
    // transcripts alone.
    let scratch = tempfile::tempdir().expect("a scratch directory");
    let root = scratch.path().join("document-parity-run");
    let transcripts = Harness::transcripts_root(&root);
    assert_eq!(
        transcripts.parent(),
        root.parent(),
        "a run's transcripts live beside its root"
    );
    std::fs::create_dir(&transcripts).expect("a transcript directory");

    let dead = scratch.path().join("document-parity-dead");
    mark_dead(&dead);
    std::thread::sleep(Duration::from_millis(20));
    let bounds = Bounds {
        grace: Duration::ZERO,
        ..Bounds::default()
    };
    let reclaimed = document_parity::supervisor::sweep_in(scratch.path(), None, &bounds);
    assert_eq!(
        reclaimed,
        vec![dead.clone()],
        "a sweep in a transcript's own directory reclaims roots and nothing else"
    );
    assert!(
        transcripts.exists(),
        "a sweep takes a root, never a transcript directory"
    );
}

#[test]
fn a_supervised_run_that_spends_its_budget_is_cut_short_without_failing_a_case() {
    // The gate's own path: a spent budget stops the run. It does not report the cases it never
    // reached as failed, because a case nobody ran says nothing about the engine.
    let _permit = common::permit();
    let harness = Harness::new_with(|_| {
        Ok((
            Box::new(HelperProbe) as Box<dyn Writer>,
            Box::new(HelperProbe) as Box<dyn Reader>,
        ))
    })
    .expect("a harness root")
    .with_budget(Duration::ZERO);
    let baseline = Baseline::load(&Baseline::default_path()).expect("a baseline");
    let supervision = Supervision::new(EngineKind::Helper, PathBuf::from(HELPER));
    let supervised = harness
        .supervise(&baseline, &mut common::NullSink, &supervision)
        .expect("a supervised run");
    assert_eq!(
        supervised.cut_short,
        Some(CutShort::Budget(Duration::ZERO)),
        "the budget cut the run short"
    );
    assert!(
        supervised.report.verdicts.is_empty() && supervised.report.readings.is_empty(),
        "a cut-short run fails nothing it did not reach"
    );
}

/// Write a marker naming this live process with a start time it does not have: the identifier is
/// live, the owner it names is gone, the same evidence a reused identifier leaves. Written by hand
/// because a claim cannot be taken on a root that already holds one.
fn mark_dead(root: &std::path::Path) {
    std::fs::create_dir_all(root).expect("a dead root");
    let marker = root.join(document_parity::supervisor::ROOT_MARKER);
    std::fs::write(marker, format!("{} 1 0\n", std::process::id())).expect("a dead marker");
}

#[test]
fn an_interrupt_kills_the_case_tree_and_exits_three() {
    for signal in [libc::SIGINT, libc::SIGTERM, libc::SIGHUP] {
        interrupt(signal);
    }
}

/// A spawned gate the test kills and reaps on drop, so a failing assertion does not leave a whole
/// supervised run behind holding the suite's concurrency permit.
struct Gate(std::process::Child);

impl std::ops::Deref for Gate {
    type Target = std::process::Child;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl std::ops::DerefMut for Gate {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

impl Drop for Gate {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

/// A path the test removes on drop, so a failing assertion leaves no file in the temp directory and
/// no root behind that the sweep would never reclaim.
struct Temp(PathBuf);

impl Drop for Temp {
    fn drop(&mut self) {
        if self.0.is_dir() {
            let _ = std::fs::remove_dir_all(&self.0);
        } else {
            let _ = std::fs::remove_file(&self.0);
        }
    }
}

/// Whether `pid` is still running, rather than a zombie its parent has not reaped: `kill(pid, 0)`
/// answers zero for a zombie too, so the process state is what decides.
fn running(pid: i32) -> bool {
    let state = std::process::Command::new("ps")
        .args(["-o", "stat=", "-p", &pid.to_string()])
        .output();
    match state {
        Ok(output) => {
            let state = String::from_utf8_lossy(&output.stdout);
            let state = state.trim();
            !state.is_empty() && !state.starts_with('Z')
        }
        Err(_) => (unsafe { libc::kill(pid, 0) }) == 0,
    }
}

/// Send `signal` to a gate whose second case hangs with a descendant in its own process group.
fn interrupt(signal: i32) {
    let _permit = common::permit();
    let cases = document_parity::corpus::writer_cases();
    assert!(cases.len() > 1, "the corpus holds at least two cases");
    // One ordinary case before the hanging one, so a verdict line exists before the signal.
    let hang = cases[1].key().slug();
    let pidfile = Temp(std::env::temp_dir().join(format!(
        "parity-helper-hang-{}-{:016x}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|duration| duration.as_nanos())
            .unwrap_or_default() as u64
    )));
    let mut child = Gate(
        std::process::Command::new(PARITY)
            .args(["--engine", &format!("helper:{HELPER}")])
            .env("PARITY_HELPER_HANG", &hang)
            .env("PARITY_HELPER_PIDFILE", &pidfile.0)
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .expect("the gate binary"),
    );
    let mut stdout = BufReader::new(child.stdout.take().expect("a pipe"));
    // The first verdict is the ordinary case before the hanging one.
    let mut first = String::new();
    let deadline = Instant::now() + Duration::from_secs(60);
    loop {
        first.clear();
        assert!(
            Instant::now() < deadline,
            "no verdict line before the deadline"
        );
        if stdout.read_line(&mut first).expect("a line") == 0 {
            panic!("the gate closed its output before a verdict");
        }
        if first.starts_with("PASS ") || first.starts_with("FAIL ") {
            break;
        }
    }
    // Readiness is observed, not assumed: the hanging case has spawned its own-group descendant.
    let deadline = Instant::now() + Duration::from_secs(60);
    let descendant: i32 = loop {
        if let Ok(text) = std::fs::read_to_string(&pidfile.0)
            && let Ok(pid) = text.trim().parse()
        {
            break pid;
        }
        assert!(
            Instant::now() < deadline,
            "the hanging case never wrote its descendant's pid"
        );
        std::thread::sleep(Duration::from_millis(50));
    };
    // The signal reaches the gate, whose handler only records it; the ordinary path tears the tree
    // down and exits with the cut-short code.
    let sent = unsafe { libc::kill(child.id() as i32, signal) };
    assert_eq!(sent, 0, "the gate accepted signal {signal}");
    let status = child.wait().expect("the gate exited");
    assert_eq!(
        status.code(),
        Some(3),
        "a cut-short run exits three after signal {signal}"
    );
    // The reason is on standard error, where the report's verdicts cannot be confused with it.
    let mut stderr = String::new();
    child
        .stderr
        .take()
        .expect("the gate's error pipe")
        .read_to_string(&mut stderr)
        .expect("the gate's error output");
    assert!(
        stderr.contains(&format!("parity: cut short: signal {signal} arrived")),
        "a cut-short run says why on standard error: {stderr}"
    );
    // The descendant in its own process group must not outlive the interrupt.
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if !running(descendant) {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "the descendant in its own process group outlived signal {signal}"
        );
        std::thread::sleep(Duration::from_millis(50));
    }
    // The cut-short run is visibly cut short by the absence of the aggregate summary line.
    let mut rest = String::new();
    stdout
        .read_to_string(&mut rest)
        .expect("the rest of the output");
    assert!(
        !rest.contains("verdicts ("),
        "a cut-short run prints no aggregate summary: {rest}"
    );
    assert!(
        !rest.contains("pairs ("),
        "a cut-short run prints no pair table: {rest}"
    );
}

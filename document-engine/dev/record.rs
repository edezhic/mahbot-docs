//! Record the baseline from the engine's own writer and reader. A deliberate act: the harness never
//! rewrites the baseline on its own, so a difference is always something a person chose to accept.
//!
//! The previous baseline is loaded and a reviewable diff is printed before the file is written: the
//! counts and names of every added, removed and changed case and reading, and whether either
//! identity moved. Recording from the engine against itself is what makes the baseline a record of
//! last time's behaviour rather than another implementation's.

use crate::baseline::Baseline;
use crate::engine::{Reader, Writer};
use crate::harness::Harness;
use crate::modes::{RECORD_ENV, err, out, recording_permitted, value};
use crate::reference::{CrateReader, CrateWriter};
use crate::transcript::identity_line;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// How many names one category prints before the line says how many were not printed.
const NAMES_PRINTED: usize = 40;

/// What one recording asks for. Parsed before any work, so a malformed invocation is refused rather
/// than after the corpus has run: an empty path would write the baseline into the working directory
/// and fail to rename it.
#[derive(Debug)]
pub(crate) struct Options {
    path: PathBuf,
}

impl Options {
    /// `Ok(None)` means `--help` was asked for and the usage has been printed.
    pub(crate) fn parse(args: &[String]) -> Result<Option<Self>, String> {
        let mut options = Self {
            path: Baseline::default_path(),
        };
        let mut args = args.iter();
        while let Some(arg) = args.next() {
            match arg.as_str() {
                "--baseline" => {
                    options.path =
                        PathBuf::from(value(args.next().map(String::as_str), "--baseline")?)
                }
                "--help" | "-h" => {
                    out(USAGE);
                    return Ok(None);
                }
                other => return Err(format!("unknown argument: {other}")),
            }
        }
        Ok(Some(options))
    }
}

const USAGE: &str = "\
usage: DOCUMENT_ENGINE_RECORD=1 cargo test -p document-engine --test gate \
-- --ignored --exact record_baseline -- [--baseline <path>]
Runs the engine's own writer and reader over the whole corpus, prints what changed against the
baseline on disk, and writes the baseline.";

pub fn run(args: &[String]) -> i32 {
    let options = match start(args, recording_permitted()) {
        Ok(Some(options)) => options,
        Ok(None) => return 0,
        Err(e) => {
            err(&e);
            return 2;
        }
    };
    if let Err(e) = record(&options.path) {
        err(&format!("record_baseline: {e:#}"));
        return 2;
    }
    0
}

/// Everything decided before any work: `--help`, then the opt-in, then the arguments.
///
/// A pure function of its arguments and the permission, so a test proves the refusal without running
/// the corpus and without touching the baseline. `Ok(None)` means `--help` was asked for; the opt-in
/// is checked after it, so help answers without the variable.
pub(crate) fn start(args: &[String], permitted: bool) -> Result<Option<Options>, String> {
    let Some(options) = Options::parse(args).map_err(|e| format!("record_baseline: {e}"))? else {
        return Ok(None);
    };
    if !permitted {
        return Err(format!(
            "record_baseline: refusing to rewrite the committed baseline: set {RECORD_ENV}=1 to mean it"
        ));
    }
    Ok(Some(options))
}

fn record(path: &Path) -> anyhow::Result<()> {
    let harness = Harness::new_with(|_| {
        Ok((
            Box::new(CrateWriter) as Box<dyn Writer>,
            Box::new(CrateReader) as Box<dyn Reader>,
        ))
    })?;
    let run = harness.run();
    // A run that could not produce a transcript is a broken harness or a broken engine; recording
    // it would bake the failure into the baseline.
    let mut broken: Vec<String> = run
        .outcomes
        .iter()
        .filter_map(|outcome| {
            outcome
                .problem
                .as_ref()
                .map(|problem| format!("{}: {problem}", outcome.key))
        })
        .collect();
    broken.extend(run.readings.iter().filter_map(|outcome| {
        outcome
            .problem
            .as_ref()
            .map(|problem| format!("reading {}: {problem}", outcome.document))
    }));
    if !broken.is_empty() {
        anyhow::bail!(
            "refusing to record a run that did not complete:\n  {}",
            broken.join("\n  ")
        );
    }
    let baseline = Baseline::record(&run)?;
    match Baseline::load(path) {
        Ok(previous) => {
            for line in review(&previous, &baseline) {
                out(&line);
            }
        }
        Err(_) => out(&format!(
            "no readable baseline at {}: recording from scratch",
            path.display()
        )),
    }
    baseline.save(path)?;
    out(&format!(
        "recorded {} cases and {} readings",
        baseline.cases.len(),
        baseline.readings.len()
    ));
    out(&format!("writer   {}", identity_line(&baseline.writer)));
    out(&format!("reader   {}", identity_line(&baseline.reader)));
    out(&format!("baseline {}", path.display()));
    Ok(())
}

/// The reviewable difference between the baseline on disk and the one about to be written.
fn review(previous: &Baseline, recorded: &Baseline) -> Vec<String> {
    let mut lines = vec!["reviewing the new baseline against the one on disk".to_string()];
    let (added, removed, changed) = changes(&previous.cases, &recorded.cases);
    lines.push(format!(
        "cases: {} added, {} removed, {} changed",
        added.len(),
        removed.len(),
        changed.len()
    ));
    list("added cases", &added, &mut lines);
    list("removed cases", &removed, &mut lines);
    list("changed cases", &changed, &mut lines);
    let (added, removed, changed) = changes(&previous.readings, &recorded.readings);
    lines.push(format!(
        "readings: {} added, {} removed, {} changed",
        added.len(),
        removed.len(),
        changed.len()
    ));
    list("added readings", &added, &mut lines);
    list("removed readings", &removed, &mut lines);
    list("changed readings", &changed, &mut lines);
    lines.push(if previous.writer == recorded.writer {
        "writer identity unchanged".to_string()
    } else {
        format!(
            "writer identity moved: {} -> {}",
            identity_line(&previous.writer),
            identity_line(&recorded.writer)
        )
    });
    lines.push(if previous.reader == recorded.reader {
        "reader identity unchanged".to_string()
    } else {
        format!(
            "reader identity moved: {} -> {}",
            identity_line(&previous.reader),
            identity_line(&recorded.reader)
        )
    });
    lines
}

/// The keys added, removed and changed between two recorded maps.
fn changes<'a, T: PartialEq>(
    previous: &'a BTreeMap<String, T>,
    recorded: &'a BTreeMap<String, T>,
) -> (Vec<&'a str>, Vec<&'a str>, Vec<&'a str>) {
    let added = recorded
        .keys()
        .filter(|key| !previous.contains_key(*key))
        .map(String::as_str)
        .collect();
    let removed = previous
        .keys()
        .filter(|key| !recorded.contains_key(*key))
        .map(String::as_str)
        .collect();
    let changed = recorded
        .iter()
        .filter(|(key, value)| previous.get(*key).is_some_and(|old| old != *value))
        .map(|(key, _)| key.as_str())
        .collect();
    (added, removed, changed)
}

/// One category's names, capped, with the count of the names the cap left out.
fn list(what: &str, names: &[&str], lines: &mut Vec<String>) {
    if names.is_empty() {
        return;
    }
    lines.push(format!("  {what} ({}):", names.len()));
    for name in names.iter().take(NAMES_PRINTED) {
        lines.push(format!("    {name}"));
    }
    if names.len() > NAMES_PRINTED {
        lines.push(format!(
            "    ... and {} more not printed",
            names.len() - NAMES_PRINTED
        ));
    }
}

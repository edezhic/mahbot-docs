//! The machinery a document chain shares: the generated files it embeds, the steps it runs, and the
//! substitution that puts the run's own paths in place of `"$previous"` and `"$asset:<name>"`.
//!
//! The record's corpus and the comparison's fixed set keep their own document lists — one covers
//! every document the record states, the other the five PDFs the comparison judges — but a chain is
//! produced the same way on both sides, so the rule lives here once.

use std::path::{Path, PathBuf};

use serde_json::Value;

/// One step of a document's chain: an operation and the values it takes.
pub struct Step {
    /// The operation's name, as [`document_engine::operation`] spells it.
    pub operation: &'static str,
    /// The values, in the operation's own terms. Two placeholders stand for a path the run decides:
    /// `"$previous"` is the file the previous step produced, and `"$asset:<name>"` is a generated
    /// asset.
    pub values: fn() -> Value,
}

/// One generated file a chain embeds.
pub struct Asset {
    /// The name its `"$asset:<name>"` placeholder states.
    pub name: &'static str,
    /// The file's extension. It is stated per asset, so adding one cannot silently inherit another's.
    pub extension: &'static str,
    /// Its bytes.
    pub bytes: fn() -> Vec<u8>,
}

/// Every generated file a chain embeds, written once per run.
pub struct Assets {
    files: Vec<(&'static str, PathBuf)>,
}

impl Assets {
    /// Write `assets` into `directory` and answer with their paths.
    ///
    /// # Errors
    ///
    /// The directory or a file cannot be written.
    pub fn write(directory: &Path, assets: &[Asset]) -> Result<Self, String> {
        std::fs::create_dir_all(directory)
            .map_err(|error| format!("cannot create {}: {error}", directory.display()))?;
        let mut files = Vec::with_capacity(assets.len());
        for asset in assets {
            let path = directory.join(format!("{}.{}", asset.name, asset.extension));
            std::fs::write(&path, (asset.bytes)())
                .map_err(|error| format!("cannot write {}: {error}", path.display()))?;
            files.push((asset.name, path));
        }
        Ok(Self { files })
    }

    /// The path a `"$asset:<name>"` placeholder stands for.
    fn path(&self, name: &str) -> Option<&Path> {
        self.files
            .iter()
            .find(|(known, _)| *known == name)
            .map(|(_, path)| path.as_path())
    }
}

/// One chain as the engine produced it.
pub struct Produced {
    /// The file the last step wrote.
    pub path: PathBuf,
    /// Its bytes.
    pub bytes: Vec<u8>,
}

/// Produce one chain under `directory` and answer what came out.
///
/// `base_name` decides the name each step writes under. Every step has its own, because the engine
/// never overwrites and a step that read the previous one's file would otherwise collide with it.
/// `record` sees each step's declared values exactly once, before substitution, so a record and the
/// request it describes cannot drift apart.
///
/// # Errors
///
/// The directory cannot be created, a step is refused by the engine, a step writes no file or more
/// than one, or the produced file cannot be read.
pub fn build(
    id: &str,
    steps: &[Step],
    directory: &Path,
    base_name: impl Fn(usize) -> String,
    assets: &Assets,
    mut record: impl FnMut(&str, &Value),
) -> Result<Produced, String> {
    std::fs::create_dir_all(directory)
        .map_err(|error| format!("cannot create {}: {error}", directory.display()))?;
    let mut previous: Option<PathBuf> = None;
    for (index, step) in steps.iter().enumerate() {
        let declared = (step.values)();
        record(step.operation, &declared);
        let values = substitute(declared, previous.as_deref(), assets)?;
        let request = document_engine::operation::Request {
            operation: step.operation.to_string(),
            values,
            destination: document_engine::operation::Destination {
                directory: directory.to_path_buf(),
                base_name: base_name(index),
            },
        };
        let outcome = document_engine::operation::execute(&request);
        if let Some(failure) = outcome.failure {
            return Err(format!(
                "{id}: step {} ({}) was refused as {}: {}",
                index + 1,
                step.operation,
                failure.class,
                failure.reason
            ));
        }
        let [output] = outcome.outputs.as_slice() else {
            return Err(format!(
                "{id}: step {} ({}) wrote {} files, and a chain expects one",
                index + 1,
                step.operation,
                outcome.outputs.len()
            ));
        };
        previous = Some(output.clone());
    }
    let path = previous.ok_or_else(|| format!("{id}: the chain declares no step"))?;
    let bytes =
        std::fs::read(&path).map_err(|error| format!("cannot read {}: {error}", path.display()))?;
    Ok(Produced { path, bytes })
}

/// The declared values with the run's own paths put in place of the two placeholders.
fn substitute(
    value: Value,
    previous: Option<&Path>,
    assets: &Assets,
) -> Result<serde_json::Map<String, Value>, String> {
    let filled = fill(value, previous, assets)?;
    filled
        .as_object()
        .cloned()
        .ok_or_else(|| "a step's values must be an object".to_string())
}

/// Replace every placeholder in `value`, however deep.
fn fill(value: Value, previous: Option<&Path>, assets: &Assets) -> Result<Value, String> {
    Ok(match value {
        Value::String(text) => match text.strip_prefix("$asset:") {
            Some(name) => Value::String(
                assets
                    .path(name)
                    .ok_or_else(|| format!("the chain declares no asset {name}"))?
                    .to_string_lossy()
                    .into_owned(),
            ),
            None if text == "$previous" => Value::String(
                previous
                    .ok_or_else(|| "a step reads $previous but no step produced one".to_string())?
                    .to_string_lossy()
                    .into_owned(),
            ),
            None => Value::String(text),
        },
        Value::Array(items) => Value::Array(
            items
                .into_iter()
                .map(|item| fill(item, previous, assets))
                .collect::<Result<_, _>>()?,
        ),
        Value::Object(map) => Value::Object(
            map.into_iter()
                .map(|(key, item)| Ok((key, fill(item, previous, assets)?)))
                .collect::<Result<_, String>>()?,
        ),
        other => other,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    static CALLS: AtomicUsize = AtomicUsize::new(0);

    fn counting() -> Value {
        CALLS.fetch_add(1, Ordering::SeqCst);
        serde_json::json!({"op": "create", "format": "pdf", "content": []})
    }

    #[test]
    fn a_step_is_evaluated_once_for_the_record_and_the_request() {
        CALLS.store(0, Ordering::SeqCst);
        let area = crate::TempArea::new("chain-once").unwrap();
        let assets = Assets::write(&area.path().join("assets"), &[]).unwrap();
        let steps = [Step {
            operation: "create",
            values: counting,
        }];
        let mut recorded = Vec::new();
        let produced = build(
            "pdf/once",
            &steps,
            area.path(),
            |_| "once".to_string(),
            &assets,
            |operation, values| recorded.push((operation.to_string(), values.clone())),
        )
        .unwrap();
        assert_eq!(CALLS.load(Ordering::SeqCst), 1);
        assert_eq!(recorded.len(), 1);
        assert!(produced.bytes.starts_with(b"%PDF"));
    }
}

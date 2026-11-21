//! Every write into a dictionary goes through the object graph's own helper, and this is the check.
//!
//! A value read from a body may be nested tens of thousands of levels deep, and the object model's own
//! `Dictionary::set` drops the value it replaces with the language's own recursion — which aborts the
//! process, an outcome the engine's panic containment cannot catch. `graph::set` dismantles it on the
//! heap instead. The two are indistinguishable at a call site: a dictionary the engine just built and
//! one it read from a file have the same type. So the family never calls the object model's `set`, and
//! this test is what keeps it that way — `graph.rs` is the one home of the write.
//!
//! The document's own object map is the same hazard, and the same two homes: `graph.rs`, whose
//! `Document` writes it with a helper that dismantles what it replaces, and `scan.rs`, where the walk
//! builds the map and dismantles what it replaces. A borrow of a value already in the map —
//! `objects.get_mut` — replaces nothing, so it is not a write and this scan does not forbid it; a
//! replacement through one would be, and none is made.
//!
//! What this scan enforces is exactly those two writes. It does **not** enforce the family's other half
//! of the rule — that no value a file states is ever cloned or dropped with the language's own
//! recursion — because a source scan cannot see an implicit clone or drop: a `Vec<Object>`, an
//! `Option<Object>` or a `Stream` dropped by going out of scope contains no call to look for. That half
//! is carried by a case per path in `writing::pdf::tests`, each on the harness's two-mebibyte stack;
//! `graph.rs` states the rule, what guards it and what cannot.

// Not every helper is used here; they are shared with the other test targets.
#[expect(dead_code, reason = "shared with the crate's other test targets")]
mod common;

/// The family's sources, with the test module removed: a test builds its fixtures with the object
/// model's own methods, which is not a write into a document the engine read. The module is the last
/// item of every file that has one, so it is cut from its own attribute to the end.
fn sources() -> Vec<(String, String)> {
    let directory = common::root().join("src").join("writing").join("pdf");
    common::rust_files(&directory)
        .into_iter()
        .map(|path| {
            let text = std::fs::read_to_string(&path).expect("a source the crate compiles");
            let code = match text.find("#[cfg(test)]\nmod ") {
                Some(start) => text[..start].to_string(),
                None => text,
            };
            (
                path.file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .into_owned(),
                code,
            )
        })
        .collect()
}

/// Every call to `call` outside the modules `homes` names, as `file:line: text`.
fn findings(call: &str, homes: &[&str]) -> Vec<String> {
    let mut found = Vec::new();
    for (name, code) in sources() {
        if homes.contains(&name.as_str()) {
            continue;
        }
        for (index, line) in code.lines().enumerate() {
            if line.trim_start().starts_with("//") {
                continue;
            }
            if line.contains(call) {
                found.push(format!("{name}:{}: {}", index + 1, line.trim()));
            }
        }
    }
    found
}

#[test]
fn no_dictionary_is_written_with_the_object_models_own_set() {
    let found = findings(".set(", &["graph.rs"]);
    assert!(
        found.is_empty(),
        "a dictionary is written with the object model's own `set`, which drops the value it replaces \
         by recursing: use `graph::set`. Sites:\n{}",
        found.join("\n")
    );
}

#[test]
fn the_object_map_is_written_in_one_of_its_two_homes() {
    // `graph.rs` states a new number; `scan.rs` builds the map as it walks, and both dismantle what
    // they replace. Anywhere else is a write into a document whose replaced object is dropped by the
    // language's own recursion.
    for call in [".objects.insert", ".objects.remove"] {
        let found = findings(call, &["graph.rs", "scan.rs"]);
        assert!(
            found.is_empty(),
            "the object map is written outside its homes: use `graph`. Sites:\n{}",
            found.join("\n")
        );
    }
}

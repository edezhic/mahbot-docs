//! The layout's disposition table against the committed module list, in the isolated copy.
//!
//! The pinned library's bundle is not a repository artifact, so the module list it names is committed
//! as data beside the width data and this test walks it against the table in both directions: every
//! module the list holds has a row, and every row names a module the list holds. The other direction
//! — every module in the package appearing in the list — was checked by the derivation command, which
//! retired with the kit; the list is a frozen expectation now, so nothing re-derives it.
//!
//! What the rows say is not machine-checkable, and this test does not pretend otherwise. What it
//! does check is decided by each row's own status and probe rather than by the words of its note: a
//! full port states no note and names no probe, a row that is not one states both, and every probe
//! the crate ships is named by a row.

use document_engine::test_support::{Disposition, docx_layout_disposition};

#[test]
fn every_module_the_json_list_carries_has_a_row_and_every_row_names_a_module() {
    let (table, modules, runtime) = docx_layout_disposition();
    let mut listed: Vec<&str> = modules.iter().map(String::as_str).collect();
    listed.sort_unstable();
    let mut unique = listed.clone();
    unique.dedup();
    assert_eq!(listed, unique, "the module list states a module twice");

    let mut rows: Vec<&str> = table.iter().map(|row| row.module).collect();
    rows.sort_unstable();
    let mut rows_unique = rows.clone();
    rows_unique.dedup();
    assert_eq!(rows, rows_unique, "the table states a module twice");

    assert_eq!(
        listed, rows,
        "the table and the module list name different modules"
    );
    // The bundler's own runtime helpers are not source modules, and the list excludes them by name:
    // they live in the JSON's own `runtime_markers` field.
    for marker in &runtime {
        assert!(
            !modules.contains(marker),
            "{marker} is a runtime marker, not a module"
        );
    }
    assert_eq!(
        modules.len(),
        30,
        "the bundle carries thirty source modules"
    );
    assert_eq!(runtime.len(), 7, "the bundle carries seven runtime markers");
}

#[test]
fn a_row_that_is_not_a_full_port_names_a_probe_the_crate_ships_and_every_probe_is_named() {
    let (table, _, _) = docx_layout_disposition();
    let probes = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("assets/docx-layout/probes");
    let mut named = Vec::new();
    for row in table {
        // The row's status and its probe decide, not the words of its note: a full port has no gap, so
        // it states no note and rests on no probe, and every other row states both.
        match row.disposition {
            Disposition::Ported => {
                assert!(
                    row.note.is_none(),
                    "{} is a full port but states a note: {:?}",
                    row.module,
                    row.note
                );
                assert!(
                    row.probe.is_none(),
                    "{} is a full port and names {:?}",
                    row.module,
                    row.probe
                );
            }
            Disposition::Partly | Disposition::Excluded => {
                assert!(
                    row.note.is_some(),
                    "{} is not a full port and states no note",
                    row.module
                );
                let probe = row.probe.unwrap_or_else(|| {
                    panic!("{} is not a full port and names no probe", row.module)
                });
                assert!(
                    probes.join(probe).exists(),
                    "{} names {probe}, which the crate does not ship",
                    row.module
                );
                named.push(probe);
            }
        }
    }
    let mut shipped: Vec<String> = std::fs::read_dir(&probes)
        .expect("the probe scripts are shipped beside the data")
        .map(|entry| {
            entry
                .expect("a probe script")
                .file_name()
                .to_string_lossy()
                .into_owned()
        })
        .filter(|name| name.ends_with(".mjs") && name != "lib.mjs")
        .collect();
    shipped.sort_unstable();
    named.sort_unstable();
    named.dedup();
    assert_eq!(
        shipped, named,
        "a probe script no row names, or a row naming a probe that is not shipped"
    );
}

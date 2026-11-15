//! The layout's numeric constants, read from the data the derivation command writes.
//!
//! Every constant is stated once, in `assets/docx-layout/constants.json`, with the quantity it counts
//! and its unit, so the restatement scan can read the file and tell a constant that duplicates a
//! vocabulary bound from one that merely spells the same numeral in another quantity. The data is
//! the derivation command's own output; a constant the crate needs and the data does not state is a
//! defect in the derivation, which is why [`value`] answers `None` rather than a made-up number.

use std::sync::LazyLock;

use serde_json::Value;

static CONSTANTS: LazyLock<Value> = LazyLock::new(|| {
    serde_json::from_str(include_str!(
        "../../../../assets/docx-layout/constants.json"
    ))
    .expect("the layout constants are the derivation command's own output")
});

/// A constant's value, by the name the data states it under.
pub(super) fn value(name: &str) -> Option<f64> {
    CONSTANTS.get(name)?.get("value")?.as_f64()
}

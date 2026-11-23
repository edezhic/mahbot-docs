//! The deliberate re-calibration mode: measure the metric's numbers from the fixed set and print
//! them. It writes no report and is not part of a run.

use std::io::Write;

use serde::Serialize;

use crate::oracle::compare::Rendered;
use crate::oracle::compare::cases::Class;
use crate::oracle::compare::metric::{Calibration, Measurements, pixel_metric, shift};
use crate::oracle::compare::render::SideState;

/// One page's measurement, as `--measure` prints it.
#[derive(Serialize)]
struct Measurement {
    /// The page's content class.
    class: Class,
    /// The measurement itself.
    #[serde(flatten)]
    measured: Measurements,
}

/// Measure the calibration's numbers from the fixed set and print them, for a deliberate
/// re-calibration. This is a separate mode, not part of a run; a run that measures nothing is a
/// mechanism failure.
///
/// Every page of the fixed set must be measured: an incomplete set would write a calibration from
/// part of the evidence, so a document or page either side did not draw is a failure, not a skip.
///
/// # Errors
///
/// A document or page of the fixed set could not be measured, no page could be measured at all, or
/// the measurement cannot be serialised.
pub fn measure(
    engine: &[Rendered<'_>],
    outside: &[Rendered<'_>],
    calibration: &Calibration,
    out: &mut dyn Write,
) -> Result<(), String> {
    let mut measurements = Vec::new();
    for &(id, _, class, document) in engine {
        // The engine must have drawn every page it holds: an unreadable document has no pages here,
        // and measuring the rest would write a calibration from part of the fixed set.
        if document.state != SideState::Opened {
            return Err(format!("{id}: the engine did not open the document"));
        }
        if document.pages.is_empty() {
            return Err(format!("{id}: the engine holds no page to measure"));
        }
        let other = outside
            .iter()
            .find(|(other, _, _, _)| *other == id)
            .map(|(_, _, _, document)| *document)
            .ok_or_else(|| format!("{id}: the outside tool answered for no such document"))?;
        for engine_page in &document.pages {
            let number = engine_page.page;
            let outside_page = other.page(number).ok_or_else(|| {
                format!("{id} page {number}: the outside tool answered for no such page")
            })?;
            let (Some(engine), Some(outside)) =
                (engine_page.drawn.picture(), outside_page.drawn.picture())
            else {
                return Err(format!("{id} page {number}: a side did not draw the page"));
            };
            let (real, total, _) = pixel_metric(
                engine.rgba,
                engine.width,
                engine.height,
                outside.rgba,
                outside.width,
                outside.height,
                &calibration.metric,
            );
            let blocks = |rgba: &[u8]| {
                pixel_metric(
                    engine.rgba,
                    engine.width,
                    engine.height,
                    rgba,
                    outside.width,
                    outside.height,
                    &calibration.metric,
                )
                .0
            };
            let noise = noisy(outside.rgba, 16);
            let sub = shift(outside.rgba, outside.width, outside.height, 0.5);
            let whole = shift(outside.rgba, outside.width, outside.height, 1.0);
            let moved = shift(
                outside.rgba,
                outside.width,
                outside.height,
                calibration.controls.element_moved_px as f32,
            );
            let blank = vec![255u8; (outside.width * outside.height * 4) as usize];
            measurements.push(Measurement {
                class,
                measured: Measurements {
                    page: format!("{id} page {number}"),
                    total_blocks: total,
                    real_pair: real,
                    noise_amplitude_16: blocks(&noise),
                    sub_pixel_shift: blocks(&sub),
                    whole_pixel_shift: blocks(&whole),
                    element_moved: blocks(&moved),
                    blank: blocks(&blank),
                },
            });
        }
    }
    if measurements.is_empty() {
        return Err("no page of the fixed set could be measured".to_string());
    }
    let text = serde_json::to_string_pretty(&measurements)
        .map_err(|error| format!("cannot write the measurement: {error}"))?;
    let _ = writeln!(out, "{text}");
    Ok(())
}

/// A deterministic ±`amplitude` noise over every channel.
fn noisy(rgba: &[u8], amplitude: i32) -> Vec<u8> {
    let mut out = rgba.to_vec();
    let mut state = 0x1234_5678u32;
    for pixel in out.as_chunks_mut::<4>().0 {
        for value in &mut pixel[..3] {
            state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            let delta = (state >> 16) as i32 % (2 * amplitude + 1) - amplitude;
            *value = (i32::from(*value) + delta).clamp(0, 255) as u8;
        }
    }
    out
}

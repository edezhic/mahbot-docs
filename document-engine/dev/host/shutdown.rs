//! The host's copy of the product's panic containment. The product suppresses its own panic hook
//! around a contained panic; here that suppression is the point: the corpus holds documents that
//! deliberately make the parser panic, and their traces on the harness's stderr would read as
//! harness failures rather than as the contained panics they are.

use std::cell::Cell;
use std::sync::Once;

thread_local! {
    static CONTAINED: Cell<bool> = const { Cell::new(false) };
}

static HOOK: Once = Once::new();

/// Install the filtering hook once per process. A panic raised inside a contained region prints
/// nothing — the caller records it as a note — while every other thread's panic prints as usual.
fn install_hook() {
    HOOK.call_once(|| {
        let previous = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            if !CONTAINED.with(Cell::get) {
                previous(info);
            }
        }));
    });
}

pub(crate) fn contain_panics<T>(f: impl FnOnce() -> T) -> std::thread::Result<T> {
    install_hook();
    let previous = CONTAINED.with(|flag| flag.replace(true));
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(f));
    CONTAINED.with(|flag| flag.set(previous));
    result
}

/// The message a caught panic carried. Against another engine implementation its own assertion
/// text is the best diagnostic there is, so it is quoted rather than replaced by "it panicked".
pub(crate) fn panic_message(payload: &(dyn std::any::Any + Send)) -> String {
    payload
        .downcast_ref::<&str>()
        .map(|text| (*text).to_string())
        .or_else(|| payload.downcast_ref::<String>().cloned())
        .unwrap_or_else(|| "a panic without a message".to_string())
}

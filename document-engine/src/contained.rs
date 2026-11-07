//! Containing a third-party decoder's panic: a malformed document is a reading, not a crash report
//! in the caller's log.
//!
//! A panic inside a contained region is caught and prints nothing — the caller reports it as a
//! note, a skipped artifact, or an unreadable file — while every other thread's panic prints as
//! usual. The flag is thread-local, so two callers never silence each other.

use std::cell::Cell;
use std::sync::Once;

thread_local! {
    /// Whether this thread is inside a contained region.
    static CONTAINED: Cell<bool> = const { Cell::new(false) };
}

static HOOK: Once = Once::new();

/// The reason the engine gives when its own code panics: a defect in the engine, never a document
/// condition.
pub(crate) const ENGINE_FAILED: &str = "the document engine failed unexpectedly";

/// Install the filtering hook once per process: a panic raised inside a contained region prints
/// nothing, while every other thread's panic prints as usual.
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

/// Run `f`, returning its result or the panic it raised. The panic is contained: it neither
/// unwinds past this call nor prints a report.
pub(crate) fn contain_panics<T>(f: impl FnOnce() -> T) -> std::thread::Result<T> {
    install_hook();
    let previous = CONTAINED.with(|flag| flag.replace(true));
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(f));
    CONTAINED.with(|flag| flag.set(previous));
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_contained_panic_neither_escapes_nor_stays_contained() {
        assert_eq!(contain_panics(|| 3).ok(), Some(3));
        assert!(contain_panics(|| panic!("contained")).is_err());
        // The flag is restored, so a later panic on this thread prints as usual.
        assert!(!CONTAINED.with(Cell::get));
        assert_eq!(contain_panics(|| 4).ok(), Some(4));
    }
}

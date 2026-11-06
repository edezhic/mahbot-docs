//! The host's copy of the product's panic containment: the product suppresses its own panic hook
//! around a contained panic, which the host does not install, so the flag is all that is left out.

thread_local! {
    static CONTAINED: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

pub(crate) fn contain_panics<T>(f: impl FnOnce() -> T) -> std::thread::Result<T> {
    let previous = CONTAINED.with(|flag| flag.replace(true));
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(f));
    CONTAINED.with(|flag| flag.set(previous));
    result
}

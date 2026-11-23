//! How a hidden mode is reached: the argument shapes both development-only test targets — the gate
//! and the oracle — select one of their modes by, and the opt-in a mode that rewrites a committed
//! record requires.
//!
//! A test target has one root, so each of the two targets declares this module beside its own root
//! and gets the same shapes rather than a copy that drifts.

use std::io::Write as _;

/// Write one line of a report to standard output, past libtest's capture.
///
/// `println!` goes through the capture libtest installs around every test, so a line written that way
/// would be swallowed unless the invocation also passed `--nocapture`. Everything a hidden mode
/// reports, and the gate's own per-pair block, is written through this instead, and reaches the
/// terminal — or the parent's pipe — however it was invoked.
pub(crate) fn out(line: &str) {
    print(&format!("{line}\n"));
}

/// Write `text` to standard output exactly, past libtest's capture.
pub(crate) fn print(text: &str) {
    let _ = write!(std::io::stdout(), "{text}");
}

/// The same to standard error: a mode's refusal must not be captured either.
pub(crate) fn err(line: &str) {
    let _ = writeln!(std::io::stderr(), "{line}");
}

/// The value after a flag. A flag with no value, an empty one, or another flag where its value
/// belongs is a usage error rather than an empty path or a path named `--quiet`.
pub(crate) fn value(argument: Option<&str>, flag: &str) -> Result<String, String> {
    match argument {
        Some(value) if !value.is_empty() && !value.starts_with('-') => Ok(value.to_string()),
        _ => Err(format!("{flag} needs a value")),
    }
}

/// The arguments after libtest's `--` separator: a hidden mode's own arguments.
pub(crate) fn mode_arguments() -> Vec<String> {
    std::env::args()
        .skip_while(|argument| argument != "--")
        .skip(1)
        .collect()
}

/// The arguments that select one hidden mode of this binary, before that mode's own arguments.
///
/// `--nocapture` is part of the prefix, though a mode's own report does not need it: a panic's
/// message must reach the parent rather than be captured and lost when the mode exits.
pub(crate) fn mode_prefix(mode: &str) -> Vec<String> {
    vec![
        "--nocapture".to_string(),
        "--ignored".to_string(),
        "--exact".to_string(),
        mode.to_string(),
        "--".to_string(),
    ]
}

/// The command that re-invokes this test binary in one of its hidden modes, with `args` after `--`.
pub(crate) fn hidden_mode(mode: &str, args: &[&str]) -> std::process::Command {
    let mut command =
        std::process::Command::new(std::env::current_exe().expect("the test binary's own path"));
    command.args(mode_prefix(mode));
    command.args(args);
    command
}

/// The variable that means "yes, rewrite the committed record".
pub(crate) const RECORD_ENV: &str = "DOCUMENT_ENGINE_RECORD";

/// Whether the caller deliberately asked to rewrite a committed record.
///
/// `--ignored` reaches every hidden mode, so a routine `cargo test -- --ignored` must not rewrite a
/// 35 MB baseline or a committed record: the variable is the difference between reaching the mode
/// and meaning it.
pub(crate) fn recording_permitted() -> bool {
    permitted(std::env::var_os(RECORD_ENV).as_deref())
}

/// Whether the variable's content is the deliberate opt-in: only its exact value `1` means it.
pub(crate) fn permitted(value: Option<&std::ffi::OsStr>) -> bool {
    value.is_some_and(|value| value == "1")
}

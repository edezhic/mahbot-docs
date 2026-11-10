//! The scaffolding the crate's own tests share: a scratch directory, a fixture written into one,
//! and a part read back from a produced package.

use std::path::{Path, PathBuf};

/// A temporary directory for one test, removed when the test ends.
pub(crate) fn scratch() -> tempfile::TempDir {
    tempfile::TempDir::new().expect("a temporary directory")
}

/// `bytes` written into `directory` under `name`, answering its path.
pub(crate) fn fixture(directory: &Path, name: &str, bytes: Vec<u8>) -> PathBuf {
    let path = directory.join(name);
    std::fs::write(&path, bytes).expect("a fixture");
    path
}

/// One part of the package at `path`, as text, read the way the engine reads a package.
pub(crate) fn part(path: &Path, name: &str) -> String {
    let bytes = std::fs::read(path).expect("the produced package");
    let parts = crate::writing::read_package(&bytes).expect("a produced package");
    let (_, content) = parts
        .iter()
        .find(|(part, _)| part == name)
        .expect("the package holds the part");
    String::from_utf8_lossy(content).into_owned()
}

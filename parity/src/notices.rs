// The licence notices that sit beside the crate's captured parts, and the one home of their names:
// `build.rs` includes this file (a build script cannot depend on the crate it builds) and the
// harness's comparison reads it, so the identity's exclusion and the archive comparison cannot
// disagree about which files are notices.
/// The notices by their path relative to the asset directory. A notice is not a part a deck holds,
/// so it is skipped wherever the directory is compared with an archive and left out of the
/// identity.
pub const NOTICES: &[&str] = &["pptxgenjs-LICENSE.txt"];

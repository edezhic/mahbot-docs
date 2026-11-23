// The licence notices that sit beside the crate's captured parts, and the one home of their names:
// the identity reads them, and the harness's comparison reads them, so the identity's exclusion and
// the archive comparison cannot disagree about which files are notices.
/// The notices by their path relative to the asset directory. A notice is not a part a deck holds,
/// so it is skipped wherever the directory is compared with an archive and left out of the
/// identity.
pub const NOTICES: &[&str] = &["pptxgenjs-LICENSE.txt"];

/// The notices and documentation that sit beside the layout's derived data, which are not data the
/// crate's layout reads and so are not bytes the comparison's identity covers.
pub const LAYOUT_NOTICES: &[&str] = &["LICENSE", "README.md"];

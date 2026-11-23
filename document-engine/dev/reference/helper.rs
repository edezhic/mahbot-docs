//! The parent's stand-in for the helper engine the harness's tests point a supervised run at.
//!
//! The helper binary runs the cases; this answers the identity and the coverage decision in the
//! parent, and refuses to run anything itself. It lives beside the other engine stand-ins because
//! it is one, not a process primitive.

use crate::engine::{EngineIdentity, Kind, Reader, Reading, Writer, WriterOutcome};
use crate::supervisor::helper_identity;
use anyhow::{Result, bail};
use std::path::Path;

/// The parent's stand-in for the helper engine.
pub struct HelperProbe;

impl Writer for HelperProbe {
    fn identity(&self) -> EngineIdentity {
        helper_identity()
    }

    fn run(&self, _request: &serde_json::Value, _scratch: &Path) -> Result<WriterOutcome> {
        bail!("the helper engine runs only in a child")
    }
}

impl Reader for HelperProbe {
    fn identity(&self) -> EngineIdentity {
        helper_identity()
    }

    fn read(&self, _input: &Path, _file_name: &str, _out_dir: &Path) -> Result<Reading> {
        bail!("the helper engine runs only in a child")
    }

    fn detect(&self, _input: &Path, _file_name: &str) -> Result<Kind> {
        bail!("the helper engine runs only in a child")
    }
}

use std::path::{Path, PathBuf};

use super::super::workspace_identity::WorkspaceObjectId;
#[cfg(any(target_os = "linux", target_os = "macos"))]
use super::archive::Fingerprint;
use super::archive::{Manifest, ReturnedEntry};

pub(super) fn apply_entries(
    root: &Path,
    expected_root_identity: &WorkspaceObjectId,
    expected_git_identity: &WorkspaceObjectId,
    baseline: &Manifest,
    returned: std::collections::BTreeMap<PathBuf, ReturnedEntry>,
    _byte_limit: u64,
    source_output_paths: Option<&[String]>,
) -> Result<(), String> {
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    return unix::apply(
        root,
        expected_root_identity,
        expected_git_identity,
        baseline,
        &returned,
        _byte_limit,
        source_output_paths,
    );
    #[cfg(windows)]
    return windows::apply(
        root,
        expected_root_identity,
        expected_git_identity,
        baseline,
        &returned,
        _byte_limit,
        source_output_paths,
    );
    #[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
    {
        let _ = (
            root,
            expected_root_identity,
            expected_git_identity,
            baseline,
            returned,
            source_output_paths,
        );
        Err(
            "Safe task-clone synchronization is unavailable on this platform; host changes were preserved".into(),
        )
    }
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
mod unix;

#[cfg(windows)]
mod windows;

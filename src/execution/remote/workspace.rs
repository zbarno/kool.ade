//! Validated task-clone snapshots for authenticated remote workers.

mod manifest;
mod scan;

pub use manifest::{WorkspaceEntryKindV1, WorkspaceEntryV1, WorkspaceManifestV1};

pub(super) fn verify_private_file_source(
    workspace: &crate::execution::contract::WorkspaceRef,
    root: &std::path::Path,
) -> anyhow::Result<()> {
    scan::verify_private_file_source(workspace, root)
}

/// Private-file grants are limited to paths that the ordinary workspace
/// snapshot excludes under the shared credential-name policy.
pub(super) fn validate_private_excluded_path(path: &str) -> anyhow::Result<()> {
    manifest::validate_relative_path(path)?;
    anyhow::ensure!(
        path.split('/').any(manifest::is_excluded_component),
        "private file path is not excluded from ordinary workspace staging"
    );
    Ok(())
}

use std::path::Path;

#[cfg(unix)]
mod diagnostics;
#[cfg(unix)]
mod filesystem;
#[cfg(unix)]
mod permissions;
#[cfg(unix)]
mod unix;
#[cfg(any(unix, windows))]
use super::super::archive::validate_relative;

#[cfg(windows)]
mod windows;

pub(crate) fn capture(
    root: &Path,
    expected_root_identity: &super::super::super::workspace_identity::WorkspaceObjectId,
    scope: Option<&Path>,
    byte_limit: u64,
    read_only: bool,
    entry_limit: usize,
) -> anyhow::Result<super::super::WorkspaceSnapshot> {
    capture_excluding(
        root,
        expected_root_identity,
        scope,
        byte_limit,
        read_only,
        entry_limit,
        &[],
    )
}

pub(crate) fn capture_excluding(
    root: &Path,
    expected_root_identity: &super::super::super::workspace_identity::WorkspaceObjectId,
    scope: Option<&Path>,
    byte_limit: u64,
    read_only: bool,
    entry_limit: usize,
    excluded_paths: &[String],
) -> anyhow::Result<super::super::WorkspaceSnapshot> {
    if let Some(scope) = scope
        && scope != Path::new(".")
    {
        validate_relative(scope)?;
    }
    #[cfg(all(not(unix), not(windows)))]
    {
        let _ = (
            root,
            expected_root_identity,
            scope,
            byte_limit,
            read_only,
            entry_limit,
            excluded_paths,
        );
        anyhow::bail!("Safe workspace staging is unavailable on this platform");
    }
    #[cfg(unix)]
    {
        unix::capture_excluding(
            root,
            expected_root_identity,
            scope,
            byte_limit,
            read_only,
            entry_limit,
            excluded_paths,
        )
    }
    #[cfg(windows)]
    {
        windows::capture_excluding(
            root,
            expected_root_identity,
            scope,
            byte_limit,
            read_only,
            entry_limit,
            excluded_paths,
        )
    }
}

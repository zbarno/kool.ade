use std::path::PathBuf;

use super::{PrerequisiteCode, PrerequisiteFailure};

pub(crate) fn app_staging_root() -> Result<PathBuf, PrerequisiteFailure> {
    let state_root = crate::persistence::state_root();
    let staging = state_root.join("execution-staging");
    if std::fs::symlink_metadata(&staging).is_ok_and(|metadata| metadata.file_type().is_symlink()) {
        return Err(PrerequisiteFailure {
            code: PrerequisiteCode::WorkspaceUnavailable,
            message: "App-owned execution staging cannot be a symbolic link".into(),
            retryable: false,
        });
    }
    std::fs::create_dir_all(&staging).map_err(|error| PrerequisiteFailure {
        code: PrerequisiteCode::WorkspaceUnavailable,
        message: format!("Could not prepare app-owned execution staging: {error}"),
        retryable: false,
    })?;
    let state_root = state_root.canonicalize().map_err(|_| PrerequisiteFailure {
        code: PrerequisiteCode::WorkspaceUnavailable,
        message: "Could not resolve the app-owned execution state directory".into(),
        retryable: false,
    })?;
    let staging = staging.canonicalize().map_err(|_| PrerequisiteFailure {
        code: PrerequisiteCode::WorkspaceUnavailable,
        message: "Could not resolve app-owned execution staging".into(),
        retryable: false,
    })?;
    if staging.parent() != Some(state_root.as_path()) {
        return Err(PrerequisiteFailure {
            code: PrerequisiteCode::WorkspaceUnavailable,
            message: "App-owned execution staging resolved outside the application state directory"
                .into(),
            retryable: false,
        });
    }
    Ok(staging)
}

use std::path::Path;

use crate::execution::docker::{
    discovery::{PrerequisiteCode, PrerequisiteFailure},
    prerequisites::inspect_workspace_capacity,
};

pub(super) fn operation_timeout(operation: &str) -> PrerequisiteFailure {
    failure(
        PrerequisiteCode::DaemonStopped,
        &format!("Docker Engine timed out during {operation}; verify the daemon and retry"),
        true,
    )
}

pub(super) fn daemon_storage_capacity(
    root: Option<&str>,
    rootless: bool,
    desktop: bool,
) -> (Option<u64>, bool) {
    let Some(root) = root else {
        return (None, false);
    };
    let path = Path::new(root);
    if !path.is_absolute() || !rootless && !desktop {
        return (None, false);
    }
    let capacity = inspect_workspace_capacity(path);
    (capacity.available_bytes, capacity.accessible)
}

pub(super) fn map_connect_error(error: &bollard::errors::Error) -> PrerequisiteFailure {
    if matches!(error, bollard::errors::Error::IOError { err } if err.kind() == std::io::ErrorKind::PermissionDenied)
    {
        failure(
            PrerequisiteCode::PermissionDenied,
            "Permission denied connecting to the local Docker Engine; check socket ownership and rootless daemon access",
            false,
        )
    } else {
        failure(
            PrerequisiteCode::DaemonStopped,
            "Docker Engine did not respond; start the local daemon and retry",
            true,
        )
    }
}

pub(super) fn map_api_error(error: &bollard::errors::Error) -> PrerequisiteFailure {
    if matches!(
        error,
        bollard::errors::Error::DockerResponseServerError {
            status_code: 401 | 403,
            ..
        }
    ) {
        failure(
            PrerequisiteCode::PermissionDenied,
            "Docker Engine denied the local API request; check daemon socket permissions",
            false,
        )
    } else {
        failure(
            PrerequisiteCode::DaemonStopped,
            "Docker Engine disconnected or did not answer its health request",
            true,
        )
    }
}

pub(super) fn failure(
    code: PrerequisiteCode,
    message: &str,
    retryable: bool,
) -> PrerequisiteFailure {
    PrerequisiteFailure {
        code,
        message: message.into(),
        retryable,
    }
}

use std::path::Path;

use super::super::failure;
use crate::execution::docker::discovery::{PrerequisiteCode, PrerequisiteFailure};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum RootlessSocketError {
    Missing,
    Inaccessible,
    Untrusted,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum RootlessSocketObservation {
    Owned(u32),
    Missing,
    Unknown,
}

#[cfg(unix)]
pub(super) fn validate_unix_socket(path: &Path, desktop: bool) -> Result<(), PrerequisiteFailure> {
    use std::os::unix::fs::{FileTypeExt, MetadataExt};

    let metadata = std::fs::symlink_metadata(path).map_err(|_| {
        failure(
            PrerequisiteCode::DockerMissing,
            "The selected local Docker socket is missing or unreadable",
            true,
        )
    })?;
    if metadata.file_type().is_symlink() || !metadata.file_type().is_socket() {
        return Err(failure(
            PrerequisiteCode::UntrustedEndpoint,
            "The selected Docker endpoint is not a direct Unix socket",
            false,
        ));
    }
    let expected_owner = if path == Path::new("/var/run/docker.sock") && !desktop {
        0
    } else {
        // Rootless Engine and Docker Desktop sockets belong to the operator.
        // Native Linux rootful Engines are rejected by readiness validation.
        unsafe { libc::geteuid() }
    };
    if metadata.uid() != expected_owner {
        return Err(failure(
            PrerequisiteCode::UntrustedEndpoint,
            "The selected Docker socket is not owned by the expected local user",
            false,
        ));
    }
    #[cfg(target_os = "linux")]
    if !desktop && is_current_user_rootless_socket(path, expected_owner) {
        validate_rootless_socket_at(
            path.parent().expect("rootless socket has a runtime directory"),
            path,
            expected_owner,
        )
        .map_err(|error| match error {
            RootlessSocketError::Missing => failure(
                PrerequisiteCode::DockerMissing,
                "The selected rootless Docker socket or its private runtime directory is missing",
                true,
            ),
            RootlessSocketError::Inaccessible => failure(
                PrerequisiteCode::PermissionDenied,
                "Permission denied checking the rootless Docker runtime directory or socket",
                false,
            ),
            RootlessSocketError::Untrusted => failure(
                PrerequisiteCode::UntrustedEndpoint,
                "The rootless Docker runtime directory or socket has unsafe ownership or permissions",
                false,
            ),
        })?;
    }
    Ok(())
}

#[cfg(target_os = "linux")]
fn is_current_user_rootless_socket(path: &Path, uid: u32) -> bool {
    path == Path::new(&format!("/run/user/{uid}/docker.sock"))
}

/// Check the rootless endpoint's complete filesystem trust boundary. The
/// private runtime directory prevents other users from reaching a socket that
/// may use Docker's normal 0660 mode. Only owner access or owner+group access
/// is accepted on the socket, with all group access contained by the private
/// runtime directory.
#[cfg(unix)]
pub(crate) fn validate_rootless_socket_at(
    runtime_dir: &Path,
    socket: &Path,
    expected_uid: u32,
) -> Result<(), RootlessSocketError> {
    use std::os::unix::fs::{FileTypeExt, MetadataExt};

    if expected_uid == 0
        || socket.parent() != Some(runtime_dir)
        || socket.file_name().is_none_or(|name| name != "docker.sock")
    {
        return Err(RootlessSocketError::Untrusted);
    }
    let runtime_metadata = std::fs::symlink_metadata(runtime_dir).map_err(map_socket_io_error)?;
    if runtime_metadata.file_type().is_symlink()
        || !runtime_metadata.is_dir()
        || runtime_metadata.uid() != expected_uid
        || runtime_metadata.mode() & 0o777 != 0o700
    {
        return Err(RootlessSocketError::Untrusted);
    }

    let socket_metadata = std::fs::symlink_metadata(socket).map_err(map_socket_io_error)?;
    let socket_mode = socket_metadata.mode() & 0o777;
    let permissions_valid = matches!(socket_mode, 0o600 | 0o660);
    if socket_metadata.file_type().is_symlink()
        || !socket_metadata.file_type().is_socket()
        || socket_metadata.uid() != expected_uid
        || !permissions_valid
    {
        return Err(RootlessSocketError::Untrusted);
    }
    Ok(())
}

#[cfg(unix)]
pub(crate) fn observe_rootless_socket_at(
    runtime_dir: &Path,
    socket: &Path,
    expected_uid: u32,
) -> RootlessSocketObservation {
    match validate_rootless_socket_at(runtime_dir, socket, expected_uid) {
        Ok(()) => RootlessSocketObservation::Owned(expected_uid),
        Err(RootlessSocketError::Missing) => RootlessSocketObservation::Missing,
        Err(RootlessSocketError::Inaccessible | RootlessSocketError::Untrusted) => {
            RootlessSocketObservation::Unknown
        }
    }
}

#[cfg(unix)]
fn map_socket_io_error(error: std::io::Error) -> RootlessSocketError {
    match error.kind() {
        std::io::ErrorKind::NotFound => RootlessSocketError::Missing,
        std::io::ErrorKind::PermissionDenied => RootlessSocketError::Inaccessible,
        _ => RootlessSocketError::Untrusted,
    }
}

#[cfg(all(test, target_os = "linux"))]
#[path = "socket/tests.rs"]
mod tests;

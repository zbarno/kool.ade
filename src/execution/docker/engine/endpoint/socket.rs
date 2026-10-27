use std::path::Path;

use super::super::failure;
use crate::execution::docker::discovery::{PrerequisiteCode, PrerequisiteFailure};

#[cfg(target_os = "macos")]
mod macos_acl;
#[cfg(all(target_os = "macos", test))]
use macos_acl::validate_macos_acl_fd;
#[cfg(target_os = "macos")]
use macos_acl::validate_macos_directory_acl;
#[cfg(target_os = "macos")]
use macos_acl::validate_macos_socket_acl;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum SocketFilesystemError {
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

    let expected_owner = if path == Path::new("/var/run/docker.sock") && !desktop {
        0
    } else {
        // Rootless Engine and Docker Desktop sockets belong to the operator.
        // Native Linux rootful Engines are rejected by readiness validation.
        unsafe { libc::geteuid() }
    };
    if desktop {
        validate_desktop_socket_parent_chain(path, expected_owner).map_err(
            |error| match error {
                SocketFilesystemError::Missing => failure(
                    PrerequisiteCode::DockerMissing,
                    "The Docker Desktop socket parent path is missing",
                    true,
                ),
                SocketFilesystemError::Inaccessible => failure(
                    PrerequisiteCode::PermissionDenied,
                    "Permission denied inspecting the Docker Desktop socket parent path",
                    false,
                ),
                SocketFilesystemError::Untrusted => failure(
                    PrerequisiteCode::UntrustedEndpoint,
                    "The Docker Desktop socket parent path has unsafe ownership or permissions",
                    false,
                ),
            },
        )?;
    }

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
    if metadata.uid() != expected_owner {
        return Err(failure(
            PrerequisiteCode::UntrustedEndpoint,
            "The selected Docker socket is not owned by the expected local user",
            false,
        ));
    }
    if desktop && metadata.mode() & 0o077 != 0 {
        return Err(failure(
            PrerequisiteCode::UntrustedEndpoint,
            "The Docker Desktop socket grants group or other access",
            false,
        ));
    }
    #[cfg(target_os = "macos")]
    if desktop {
        validate_macos_socket_acl(path, expected_owner).map_err(|error| match error {
            SocketFilesystemError::Missing => failure(
                PrerequisiteCode::DockerMissing,
                "The Docker Desktop socket disappeared during ACL inspection",
                true,
            ),
            SocketFilesystemError::Inaccessible => failure(
                PrerequisiteCode::PermissionDenied,
                "Permission denied inspecting the Docker Desktop socket ACL",
                false,
            ),
            SocketFilesystemError::Untrusted => failure(
                PrerequisiteCode::UntrustedEndpoint,
                "The Docker Desktop socket ACL or identity is unsafe",
                false,
            ),
        })?;
    }
    #[cfg(target_os = "linux")]
    if !desktop && is_current_user_rootless_socket(path, expected_owner) {
        validate_rootless_socket_at(
            path.parent().expect("rootless socket has a runtime directory"),
            path,
            expected_owner,
        )
        .map_err(|error| match error {
            SocketFilesystemError::Missing => failure(
                PrerequisiteCode::DockerMissing,
                "The selected rootless Docker socket or its private runtime directory is missing",
                true,
            ),
            SocketFilesystemError::Inaccessible => failure(
                PrerequisiteCode::PermissionDenied,
                "Permission denied checking the rootless Docker runtime directory or socket",
                false,
            ),
            SocketFilesystemError::Untrusted => failure(
                PrerequisiteCode::UntrustedEndpoint,
                "The rootless Docker runtime directory or socket has unsafe ownership or permissions",
                false,
            ),
        })?;
    }
    Ok(())
}

/// Reject Desktop sockets reached through a replaceable or redirected parent.
/// System ancestors may be root-owned; user ancestors must belong to the
/// operator and no ancestor may be writable by a different user.
#[cfg(unix)]
fn validate_desktop_socket_parent_chain(
    socket: &Path,
    expected_owner: u32,
) -> Result<(), SocketFilesystemError> {
    if !socket.is_absolute()
        || socket
            .components()
            .any(|component| matches!(component, std::path::Component::ParentDir))
    {
        return Err(SocketFilesystemError::Untrusted);
    }
    let parent = socket.parent().ok_or(SocketFilesystemError::Untrusted)?;
    let root = std::fs::File::open("/").map_err(map_socket_io_error)?;
    validate_desktop_directory(&root, expected_owner)?;
    let mut directories = vec![root];
    for component in parent.components() {
        match component {
            std::path::Component::RootDir => {}
            std::path::Component::Normal(name) => {
                use std::{
                    ffi::CString,
                    os::{
                        fd::{AsRawFd, FromRawFd},
                        unix::ffi::OsStrExt,
                    },
                };
                let name =
                    CString::new(name.as_bytes()).map_err(|_| SocketFilesystemError::Untrusted)?;
                let parent_fd = directories
                    .last()
                    .ok_or(SocketFilesystemError::Untrusted)?
                    .as_raw_fd();
                #[cfg(target_os = "macos")]
                let nofollow = libc::O_NOFOLLOW_ANY;
                #[cfg(not(target_os = "macos"))]
                let nofollow = libc::O_NOFOLLOW;
                let fd = unsafe {
                    libc::openat(
                        parent_fd,
                        name.as_ptr(),
                        libc::O_RDONLY | libc::O_DIRECTORY | libc::O_CLOEXEC | nofollow,
                    )
                };
                if fd < 0 {
                    return Err(map_socket_io_error(std::io::Error::last_os_error()));
                }
                let directory = unsafe { std::fs::File::from_raw_fd(fd) };
                validate_desktop_directory(&directory, expected_owner)?;
                directories.push(directory);
            }
            _ => return Err(SocketFilesystemError::Untrusted),
        }
    }
    Ok(())
}

#[cfg(unix)]
fn validate_desktop_directory(
    directory: &std::fs::File,
    expected_owner: u32,
) -> Result<(), SocketFilesystemError> {
    use std::os::unix::fs::MetadataExt;

    let metadata = directory.metadata().map_err(map_socket_io_error)?;
    if !metadata.is_dir()
        || (metadata.uid() != 0 && metadata.uid() != expected_owner)
        || metadata.mode() & 0o022 != 0
    {
        return Err(SocketFilesystemError::Untrusted);
    }
    #[cfg(target_os = "macos")]
    validate_macos_directory_acl(directory)?;
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
) -> Result<(), SocketFilesystemError> {
    use std::os::unix::fs::{FileTypeExt, MetadataExt};

    if expected_uid == 0
        || socket.parent() != Some(runtime_dir)
        || socket.file_name().is_none_or(|name| name != "docker.sock")
    {
        return Err(SocketFilesystemError::Untrusted);
    }
    let runtime_metadata = std::fs::symlink_metadata(runtime_dir).map_err(map_socket_io_error)?;
    if runtime_metadata.file_type().is_symlink()
        || !runtime_metadata.is_dir()
        || runtime_metadata.uid() != expected_uid
        || runtime_metadata.mode() & 0o777 != 0o700
    {
        return Err(SocketFilesystemError::Untrusted);
    }

    let socket_metadata = std::fs::symlink_metadata(socket).map_err(map_socket_io_error)?;
    let socket_mode = socket_metadata.mode() & 0o777;
    let permissions_valid = matches!(socket_mode, 0o600 | 0o660);
    if socket_metadata.file_type().is_symlink()
        || !socket_metadata.file_type().is_socket()
        || socket_metadata.uid() != expected_uid
        || !permissions_valid
    {
        return Err(SocketFilesystemError::Untrusted);
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
        Err(SocketFilesystemError::Missing) => RootlessSocketObservation::Missing,
        Err(SocketFilesystemError::Inaccessible | SocketFilesystemError::Untrusted) => {
            RootlessSocketObservation::Unknown
        }
    }
}

#[cfg(unix)]
fn map_socket_io_error(error: std::io::Error) -> SocketFilesystemError {
    match error.kind() {
        std::io::ErrorKind::NotFound => SocketFilesystemError::Missing,
        std::io::ErrorKind::PermissionDenied => SocketFilesystemError::Inaccessible,
        _ => SocketFilesystemError::Untrusted,
    }
}

#[cfg(all(test, any(target_os = "linux", target_os = "macos")))]
#[path = "socket/tests.rs"]
mod tests;

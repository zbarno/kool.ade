use std::path::Path;

use super::super::failure;
use crate::execution::docker::discovery::{PrerequisiteCode, PrerequisiteFailure};

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
    Ok(())
}

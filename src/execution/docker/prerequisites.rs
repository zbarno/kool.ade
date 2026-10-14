use std::path::Path;

use super::discovery::{PrerequisiteCode, PrerequisiteFailure};

pub const DEFAULT_MINIMUM_FREE_BYTES: u64 = 2 * 1024 * 1024 * 1024;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WorkspaceCapacity {
    pub accessible: bool,
    pub available_bytes: Option<u64>,
}

pub fn inspect_workspace_capacity(path: &Path) -> WorkspaceCapacity {
    let accessible = path.is_dir() && std::fs::read_dir(path).is_ok();
    let available_bytes = if accessible {
        fs2::available_space(path).ok()
    } else {
        None
    };
    WorkspaceCapacity {
        accessible,
        available_bytes,
    }
}

pub fn require_workspace_capacity(
    capacity: &WorkspaceCapacity,
    minimum_bytes: u64,
) -> Result<u64, PrerequisiteFailure> {
    if !capacity.accessible {
        return Err(PrerequisiteFailure {
            code: PrerequisiteCode::WorkspaceUnavailable,
            message: "The workspace staging directory is unavailable or unreadable".into(),
            retryable: false,
        });
    }
    let bytes = capacity
        .available_bytes
        .ok_or_else(|| PrerequisiteFailure {
            code: PrerequisiteCode::InsufficientDisk,
            message: "Free space for workspace staging could not be verified".into(),
            retryable: false,
        })?;
    if bytes < minimum_bytes {
        return Err(PrerequisiteFailure {
            code: PrerequisiteCode::InsufficientDisk,
            message: "Workspace staging has less free space than required".into(),
            retryable: true,
        });
    }
    Ok(bytes)
}

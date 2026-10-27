//! Container configuration built only from validated application policy.

use std::path::{Path, PathBuf};

use crate::execution::errors::{ExecutionError, ExecutionErrorKind};

pub(crate) mod staging;
pub(crate) use staging::WorkspaceSnapshot;
mod environment;
mod private_files;
mod workspace_identity;

pub const WORKSPACE_TARGET: &str = "/workspace";
pub const OWNER_LABEL: &str = "com.koolade.owner";
pub const EXECUTION_LABEL: &str = "com.koolade.execution";
pub const TASK_LABEL: &str = "com.koolade.task";
pub const WORKSPACE_LABEL: &str = "com.koolade.workspace";
pub const POLICY_LABEL: &str = "com.koolade.policy";
pub const LEASE_LABEL: &str = "com.koolade.lease-unix-ms";
pub(crate) const PRIVATE_FILES_TARGET: &str = "/koolade-private-files";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ValidatedWorkspace {
    root: PathBuf,
    git_metadata: PathBuf,
    root_identity: workspace_identity::WorkspaceObjectId,
    git_identity: workspace_identity::WorkspaceObjectId,
}

impl ValidatedWorkspace {
    /// Resolve and revalidate an application-owned task clone immediately before mount creation.
    /// Callers must obtain the root from the task-clone registry, never from request data.
    pub fn inspect(root: &Path) -> Result<Self, ExecutionError> {
        let canonical = root
            .canonicalize()
            .map_err(|_| denied("Task clone is unavailable"))?;
        let metadata = std::fs::symlink_metadata(&canonical)
            .map_err(|_| denied("Task clone metadata could not be verified"))?;
        if !metadata.is_dir() || workspace_identity::is_reparse_point(&metadata) {
            return Err(denied("Task clone root is not a real directory"));
        }
        let root_identity = workspace_identity::from_metadata(&metadata, &canonical)
            .ok_or_else(|| denied("Task clone identity cannot be verified on this platform"))?;
        let git_path = canonical.join(".git");
        let git_meta = std::fs::symlink_metadata(&git_path)
            .map_err(|_| denied("Task clone Git metadata is unavailable"))?;
        if !git_meta.is_dir() || workspace_identity::is_reparse_point(&git_meta) {
            return Err(denied("Task clone must contain a real .git directory"));
        }
        let git_identity = workspace_identity::from_metadata(&git_meta, &git_path)
            .ok_or_else(|| denied("Task clone Git metadata identity cannot be verified"))?;
        Ok(Self {
            root: canonical,
            git_metadata: git_path,
            root_identity,
            git_identity,
        })
    }

    pub fn revalidate(&self) -> Result<(), ExecutionError> {
        let current = Self::inspect(&self.root)?;
        if current != *self {
            return Err(denied("Task clone path changed before container launch"));
        }
        Ok(())
    }

    pub(super) fn root(&self) -> &Path {
        &self.root
    }

    fn root_identity(&self) -> &workspace_identity::WorkspaceObjectId {
        &self.root_identity
    }

    fn git_identity(&self) -> &workspace_identity::WorkspaceObjectId {
        &self.git_identity
    }

    #[cfg(test)]
    pub(super) fn git_metadata(&self) -> &Path {
        &self.git_metadata
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SandboxIdentity {
    pub owner_instance_id: String,
    pub policy_version: String,
    pub lease_expires_unix_ms: u64,
}

impl SandboxIdentity {
    pub fn validate(&self) -> Result<(), ExecutionError> {
        for value in [&self.owner_instance_id, &self.policy_version] {
            if value.is_empty() || value.len() > 128 || value.bytes().any(|b| b.is_ascii_control())
            {
                return Err(denied("Sandbox identity metadata is invalid"));
            }
        }
        if self.lease_expires_unix_ms == 0 {
            return Err(denied("Sandbox lease is invalid"));
        }
        Ok(())
    }
}

mod create_body;
pub use create_body::create_body;
#[cfg(test)]
pub(super) use create_body::create_body_with_derived_approval;
pub(super) use create_body::create_body_with_private_files;

fn denied(message: &str) -> ExecutionError {
    ExecutionError::new(ExecutionErrorKind::PolicyDenied, message, false)
}
fn now_unix_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .min(u64::MAX as u128) as u64
}

#[cfg(test)]
#[path = "sandbox/tests.rs"]
mod tests;

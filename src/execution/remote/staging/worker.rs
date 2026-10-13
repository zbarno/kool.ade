//! Authenticated worker-side workspace staging and Docker clone resolution.

use std::{
    collections::HashMap,
    fs,
    path::{Path, PathBuf},
    sync::Mutex,
};

use crate::execution::{
    contract::WorkspaceRef,
    docker::backend::TaskWorkspaceResolver,
    errors::{ExecutionError, ExecutionErrorKind},
};

use super::{StagingReceiver, TransferChunkV1, TransferManifestV1, TransferPayloadV1};

mod input;
mod materialize;
mod restore;
mod results;

pub struct WorkerStager {
    worker_id: String,
    staging_root: PathBuf,
    projects_root: PathBuf,
    receivers: Mutex<HashMap<String, StagingReceiver>>,
    workspaces: Mutex<HashMap<WorkspaceRef, StagedWorkspace>>,
}

struct StagedWorkspace {
    path: PathBuf,
    snapshot_generation: String,
}

impl WorkerStager {
    pub fn worker_id(&self) -> &str {
        &self.worker_id
    }

    pub fn has_workspace(&self, workspace: &WorkspaceRef) -> bool {
        self.workspaces
            .lock()
            .is_ok_and(|workspaces| workspaces.contains_key(workspace))
    }

    pub fn new(state_root: &Path, worker_id: String) -> anyhow::Result<Self> {
        anyhow::ensure!(valid_id(&worker_id), "worker identity is invalid");
        let staging_root = state_root.join("remote-staging");
        create_private_dir(&staging_root)?;
        let projects_root = state_root.join("projects");
        create_private_dir(&projects_root)?;
        let stager = Self {
            worker_id,
            staging_root,
            projects_root,
            receivers: Mutex::new(HashMap::new()),
            workspaces: Mutex::new(HashMap::new()),
        };
        stager.restore_workspaces()?;
        Ok(stager)
    }

    pub fn manifest(&self, transfer_id: &str) -> anyhow::Result<TransferManifestV1> {
        let receivers = self
            .receivers
            .lock()
            .map_err(|_| anyhow::anyhow!("worker staging lock is unavailable"))?;
        receivers
            .get(transfer_id)
            .map(|receiver| receiver.manifest().clone())
            .ok_or_else(|| anyhow::anyhow!("workspace transfer is unavailable"))
    }

    fn workspace_path(
        &self,
        manifest: &crate::execution::remote::WorkspaceManifestV1,
    ) -> anyhow::Result<PathBuf> {
        manifest.validate()?;
        Ok(self.workspace_path_for(&manifest.workspace, &manifest.snapshot_generation))
    }

    fn workspace_path_for(&self, workspace: &WorkspaceRef, snapshot_generation: &str) -> PathBuf {
        self.projects_root
            .join(&workspace.project_id)
            .join("task-repositories")
            .join(format!(
                "remote-{}-{snapshot_generation}",
                workspace.source_sha
            ))
            .join(&workspace.task_id)
    }
}

impl TaskWorkspaceResolver for WorkerStager {
    fn resolve_owned_clone(&self, workspace: &WorkspaceRef) -> Result<PathBuf, ExecutionError> {
        let workspaces = self.workspaces.lock().map_err(|_| denied())?;
        let path = &workspaces.get(workspace).ok_or_else(denied)?.path;
        let canonical = path.canonicalize().map_err(|_| denied())?;
        if canonical != *path || !canonical.starts_with(&self.projects_root) {
            return Err(denied());
        }
        let metadata = fs::symlink_metadata(&canonical).map_err(|_| denied())?;
        let git = fs::symlink_metadata(canonical.join(".git")).map_err(|_| denied())?;
        if !metadata.is_dir()
            || metadata.file_type().is_symlink()
            || !git.is_dir()
            || git.file_type().is_symlink()
        {
            return Err(denied());
        }
        Ok(canonical)
    }

    fn staging_root(&self) -> PathBuf {
        self.staging_root.clone()
    }
}

fn create_private_dir(path: &Path) -> anyhow::Result<()> {
    fs::create_dir_all(path)?;
    let metadata = fs::symlink_metadata(path)?;
    anyhow::ensure!(
        metadata.is_dir() && !metadata.file_type().is_symlink(),
        "worker staging path is unsafe"
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o700))?;
    }
    Ok(())
}

fn read_bounded(path: &Path, limit: u64) -> anyhow::Result<Vec<u8>> {
    let metadata = fs::symlink_metadata(path)?;
    anyhow::ensure!(
        metadata.is_file() && !metadata.file_type().is_symlink() && metadata.len() <= limit,
        "workspace result manifest is unavailable or oversized"
    );
    let bytes = fs::read(path)?;
    anyhow::ensure!(
        bytes.len() as u64 <= limit,
        "workspace result manifest is oversized"
    );
    Ok(bytes)
}

fn set_private_file(path: &Path) -> anyhow::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o600))?;
    }
    Ok(())
}

fn valid_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
}

#[cfg(test)]
#[path = "worker/tests.rs"]
mod tests;

fn denied() -> ExecutionError {
    ExecutionError::new(
        ExecutionErrorKind::PolicyDenied,
        "Remote workspace is not an app-owned staged task clone",
        false,
    )
}

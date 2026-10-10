use std::path::Path;

use crate::{
    app::session::Project,
    core::managed_workspace::{WorkspaceManager, WorkspaceManifest},
    error::AppError,
};

use super::{connect_canonical_with_key, expand_home};

/// Create and open a planning-only workspace before any code repository is
/// registered. The workspace identity remains stable across restarts.
pub fn attempt_create_workspace(title: &str) -> Result<Project, AppError> {
    let workspace = WorkspaceManager::from_state_root()
        .create(title)
        .map_err(|error| AppError::Other(format!("workspace creation failed: {error:#}")))?;
    connect_canonical_with_key(
        workspace.planning_repo,
        workspace.store,
        workspace.id.hyphenated().to_string(),
    )
}

/// Reopen a local workspace using its durable workspace UUID.
pub fn attempt_open_workspace(workspace_id: uuid::Uuid) -> Result<Project, AppError> {
    let workspace = WorkspaceManager::from_state_root()
        .open(workspace_id)
        .map_err(|error| AppError::Other(format!("workspace could not be opened: {error:#}")))?;
    connect_canonical_with_key(
        workspace.planning_repo,
        workspace.store,
        workspace_id.hyphenated().to_string(),
    )
}

/// Join an existing managed planning checkout and reuse its workspace ID.
pub fn attempt_join_workspace(raw: &str) -> Result<Project, AppError> {
    let path = expand_home(raw.trim());
    join_workspace_path(Path::new(path.as_os_str()))
}

pub(super) fn is_managed_repository(path: &Path) -> Result<bool, AppError> {
    WorkspaceManifest::is_managed_repository(path)
        .map_err(|error| AppError::Other(format!("planning manifest is invalid: {error:#}")))
}

pub(super) fn join_workspace_path(path: &Path) -> Result<Project, AppError> {
    let workspace = WorkspaceManager::from_state_root()
        .join(path)
        .map_err(|error| {
            AppError::Other(format!("planning workspace could not be joined: {error:#}"))
        })?;
    connect_canonical_with_key(
        workspace.planning_repo,
        workspace.store,
        workspace.id.hyphenated().to_string(),
    )
}

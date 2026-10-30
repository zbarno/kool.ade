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
    let value = raw.trim();
    let manager = WorkspaceManager::from_state_root();
    let workspace = if value.contains("://") || value.starts_with("git@") {
        manager.join_remote(value)
    } else {
        let path = expand_home(value);
        manager.join(Path::new(path.as_os_str()))
    }
    .map_err(|error| {
        AppError::Other(format!("planning workspace could not be joined: {error:#}"))
    })?;
    connect_canonical_with_key(
        workspace.planning_repo,
        workspace.store,
        workspace.id.hyphenated().to_string(),
    )
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

/// Wizard "new project" track: materializes the visible sibling pair and
/// reconnects through the ordinary code-root gate over the fresh checkout,
/// exercising the same managed detection a returning operator gets.
pub fn attempt_create_project(
    spec: &crate::core::managed_workspace::SiblingPairSpec,
) -> Result<Project, AppError> {
    WorkspaceManager::from_state_root()
        .create_sibling_pair(spec)
        .map_err(|error| AppError::Other(format!("new project could not be created: {error:#}")))?;
    let code = spec.parent_dir.join(&spec.code_repo_name);
    super::attempt_connect(&code.to_string_lossy())
}

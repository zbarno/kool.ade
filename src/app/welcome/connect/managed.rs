use crate::{
    app::session::Project,
    core::managed_workspace::{NewProjectSpec, WorkspaceManager},
    error::AppError,
};

use super::connect_canonical_with_key;

/// Reopen a local workspace using its durable workspace UUID.
pub fn attempt_open_workspace(workspace_id: uuid::Uuid) -> Result<Project, AppError> {
    let manager = WorkspaceManager::from_state_root();
    let workspace = manager
        .open(workspace_id)
        .map_err(|error| AppError::Other(format!("workspace could not be opened: {error:#}")))?;
    let repository = workspace
        .repositories
        .repositories
        .iter()
        .find(|repository| repository.id == crate::core::managed_workspace::PAIR_CODE_REPO_ID)
        .or_else(|| workspace.repositories.repositories.first())
        .ok_or_else(|| AppError::Other("workspace has no paired code repository".into()))?;
    let checkout = manager
        .checkout_path(workspace.id, &repository.id, &repository.remote)
        .map_err(|error| {
            AppError::Other(format!(
                "managed code checkout could not be opened: {error:#}"
            ))
        })?
        .ok_or_else(|| {
            AppError::Other("paired code repository is not cloned on this computer".into())
        })?;
    connect_canonical_with_key(
        checkout,
        workspace.store,
        workspace_id.hyphenated().to_string(),
    )
}

/// Create a brand-new sibling code and planning repository pair.
pub fn attempt_create_project(spec: &NewProjectSpec) -> Result<Project, AppError> {
    WorkspaceManager::from_state_root()
        .create_new_project(spec)
        .map_err(|error| AppError::Other(format!("new project could not be created: {error:#}")))?;
    let code = spec.parent_dir.join(&spec.code_repo_name);
    super::attempt_connect(&code.to_string_lossy())
}

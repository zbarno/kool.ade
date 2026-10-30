use crate::app::session::Project;
use crate::artifacts::planning_store::{PlanningStore, StoreMode};
use crate::core::gitops;
use crate::core::state::PlannerState;
use crate::error::AppError;
use crate::persistence::{chat_store, project_slug};
use std::ffi::OsString;
use std::path::PathBuf;

mod load;
mod managed;
use load::connect_canonical_with_key;
mod prepared;
pub use managed::{attempt_create_workspace, attempt_join_workspace, attempt_open_workspace};
pub use prepared::{PreparedConnection, prepare_open_workspace};
/// Normalize user-typed paths (`~` expansion), then load-or-bootstrap.
pub fn attempt_connect(raw: &str) -> Result<Project, AppError> {
    let canonical = canonical_repository(raw)?;
    if managed::is_managed_repository(&canonical)? {
        return managed::join_workspace_path(&canonical);
    }
    let manager = crate::core::managed_workspace::WorkspaceManager::from_state_root();
    if let Some(workspace) = manager
        .workspace_for_checkout(&canonical)
        .map_err(|error| {
            AppError::Other(format!(
                "managed workspace association is invalid: {error:#}"
            ))
        })?
    {
        return connect_canonical(canonical, workspace.store);
    }
    let store = PlanningStore::legacy_embedded(uuid::Uuid::nil(), &canonical);
    connect_canonical(canonical, store)
}

/// Connect a code checkout to an explicitly selected planning store. Managed
/// stores keep planning files in their own Git repository; private project
/// state remains tied to the code checkout.
pub fn attempt_connect_with_store(raw: &str, store: PlanningStore) -> Result<Project, AppError> {
    let canonical = canonical_repository(raw)?;
    if store.mode == StoreMode::LegacyEmbedded
        && store.root != canonical.join(crate::artifacts::layout::canonical::ROOT)
    {
        return Err(AppError::InvalidRepo {
            path: canonical.to_string_lossy().into_owned(),
            detail: "legacy planning stores must remain inside the connected code checkout".into(),
        });
    }
    connect_canonical(canonical, store)
}

fn canonical_repository(raw: &str) -> Result<PathBuf, AppError> {
    let raw = raw.trim();
    if raw.is_empty() {
        return Err(AppError::InvalidRepo {
            path: raw.to_string(),
            detail: "Please enter the path to a git repository (working tree).".into(),
        });
    }
    let expanded = expand_home(raw);
    let canonical = std::fs::canonicalize(&expanded).map_err(|e| AppError::InvalidRepo {
        path: raw.to_string(),
        detail: format!("could not open that path: {e}"),
    })?;
    if !canonical.is_dir() {
        return Err(AppError::InvalidRepo {
            path: canonical.to_string_lossy().into_owned(),
            detail: "path is not a directory".into(),
        });
    }
    // A folder picker can select a subfolder inside a checkout. Normalize it
    // to the worktree root before managed detection so legacy bootstrap never
    // creates planning files under a nested directory in a managed repo.
    Ok(crate::core::managed_workspace::repository_root(&canonical).unwrap_or(canonical))
}

fn connect_canonical(canonical: PathBuf, store: PlanningStore) -> Result<Project, AppError> {
    let local_key = if store.mode != StoreMode::LegacyEmbedded && !store.project_id.is_nil() {
        store.project_id.hyphenated().to_string()
    } else {
        project_slug(&canonical)
    };
    connect_canonical_with_key(canonical, store, local_key)
}

fn expand_home(raw: &str) -> OsString {
    if let Some(rest) = raw.strip_prefix("~/")
        && let Some(home) = std::env::var_os("HOME")
    {
        return PathBuf::from(home).join(rest).into_os_string();
    }
    raw.into()
}

use crate::app::session::Project;
use crate::artifacts::planning_store::PlanningStore;
use crate::core::gitops;
use crate::error::AppError;
use std::ffi::OsString;
use std::path::PathBuf;

mod load;
use load::connect_canonical_with_key;
/// Connect directly to a code checkout. Published project documentation is
/// read from its Git history; operator-private planning state lives under
/// KOOLADE_HOME and is scoped to this checkout.
pub fn attempt_connect(raw: &str) -> Result<Project, AppError> {
    let canonical = canonical_repository(raw)?;
    if !gitops::is_work_tree(&canonical) {
        return Err(AppError::InvalidRepo {
            path: canonical.to_string_lossy().into_owned(),
            detail: "no .git directory found \u{2014} Kool.ad/e plans inside a git working tree. Initialize one first (git init) or choose a different folder.".into(),
        });
    }
    let state_root = crate::persistence::state_root();
    if crate::persistence::is_inside_git_worktree(&state_root) {
        return Err(AppError::InvalidRepo {
            path: canonical.to_string_lossy().into_owned(),
            detail: format!(
                "private Kool.ad/e data must live outside Git worktrees; move KOOLADE_HOME outside Git (current location: {})",
                state_root.display()
            ),
        });
    }
    let slug = crate::persistence::project_slug(&canonical);
    let project_id = uuid::Uuid::new_v5(&uuid::Uuid::NAMESPACE_URL, slug.as_bytes());
    let local_root = crate::persistence::project_dir(&project_id.hyphenated().to_string())
        .join("operational-state");
    let store = PlanningStore::for_code_repository(project_id, local_root, canonical.clone());
    connect_canonical_with_key(canonical, store, slug)
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
    // to the worktree root so state uses the code repository identity.
    Ok(gitops::worktree_root(&canonical).unwrap_or(canonical))
}

fn expand_home(raw: &str) -> OsString {
    if let Some(rest) = raw.strip_prefix("~/")
        && let Some(home) = std::env::var_os("HOME")
    {
        return PathBuf::from(home).join(rest).into_os_string();
    }
    raw.into()
}

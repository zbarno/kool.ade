use std::{
    fs,
    path::Path,
    time::{SystemTime, UNIX_EPOCH},
};

use super::super::super::git;

pub(in crate::core::planning_sync::runner) fn shared_manifest(
    repo: &Path,
) -> anyhow::Result<Option<(crate::core::managed_workspace::WorkspaceManifest, bool)>> {
    if !crate::core::managed_workspace::WorkspaceManifest::is_managed_repository(repo)? {
        return Ok(None);
    }
    let manifest = crate::core::managed_workspace::WorkspaceManifest::read(repo)?;
    Ok(Some((manifest, git::remote_is_configured(repo)?)))
}

pub(in crate::core::planning_sync::runner) fn journal_lock(
    state_root: &Path,
    workspace_id: uuid::Uuid,
) -> anyhow::Result<std::fs::File> {
    let dir = state_root
        .join("projects")
        .join(workspace_id.hyphenated().to_string());
    fs::create_dir_all(&dir)?;
    let file = fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(dir.join("sync.lock"))?;
    file.lock()?;
    Ok(file)
}

pub(in crate::core::planning_sync::runner) fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| {
            duration.as_millis().min(u128::from(u64::MAX)) as u64
        })
}

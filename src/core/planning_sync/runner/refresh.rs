use std::{fs, path::Path};

use super::super::{
    journal::{OperationJournal, SyncOperation},
    merge,
    model::{PlanningSyncState, PlanningSyncStatus},
};
use super::intent::now_ms;
use super::{git, intent, reapply::transport_state};
use crate::core::managed_workspace::WorkspaceManifest;

pub(super) fn refresh_local_records(
    repo: &Path,
    operation: &SyncOperation,
    remote_tip: &str,
    later_operations: &[SyncOperation],
) -> anyhow::Result<bool> {
    let paths = git::changed_paths(repo, &operation.base_commit, remote_tip)?;
    let mut updates = Vec::new();
    for path in paths {
        if !sync_path(&path) {
            continue;
        }
        let base = git::read_path(repo, &operation.base_commit, &path)?;
        let remote = git::read_path(repo, remote_tip, &path)?;
        merge::validate_record_file(&path, remote.as_deref())
            .map_err(|_| anyhow::anyhow!("remote planning record is corrupt or incompatible"))?;
        let target = repo.join(&path);
        if !safe_target(repo, &target)? {
            return Ok(false);
        }
        let local = if target.is_file() {
            Some(fs::read(&target)?)
        } else {
            None
        };
        let operation_desired = operation
            .mutations
            .iter()
            .find(|mutation| mutation.path == path)
            .and_then(|mutation| mutation.desired.as_deref())
            .map(str::as_bytes);
        let later_desired = later_operations
            .iter()
            .rev()
            .find_map(|later| {
                later
                    .mutations
                    .iter()
                    .find(|mutation| mutation.path == path)
            })
            .map(|mutation| mutation.desired.as_deref().map(str::as_bytes));
        if later_desired.is_some_and(|desired| local.as_deref() == desired) {
            continue;
        }
        if local.as_deref() != base.as_deref() && local.as_deref() != operation_desired {
            return Ok(false);
        }
        if local != remote {
            updates.push((target, remote));
        }
    }
    for (path, content) in updates {
        match content {
            Some(bytes) => crate::artifacts::atomic_write_bytes(&path, &bytes)?,
            None => match fs::remove_file(path) {
                Ok(()) => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => return Err(error.into()),
            },
        }
    }
    Ok(true)
}

pub(super) fn refresh_workspace(
    repo: &Path,
    state_root: &Path,
    manifest: &WorkspaceManifest,
    journal: &mut OperationJournal,
) -> anyhow::Result<PlanningSyncStatus> {
    if journal.state == PlanningSyncState::IncompatibleSchema
        && journal
            .detail
            .as_deref()
            .is_some_and(|detail| detail.contains("normalized per-record"))
    {
        return Ok(journal.status());
    }
    let remote_tip = match fetch_verified(repo, manifest) {
        Ok(commit) => commit,
        Err(FetchError::Incompatible) => return incompatible_remote(journal, state_root),
        Err(FetchError::Transport(error)) => {
            journal.state = transport_state(&error.to_string());
            journal.detail = Some(error.to_string());
            journal.write(state_root)?;
            return Ok(journal.status());
        }
    };
    let base = journal.confirmed_commit.clone().unwrap_or(git::head(repo)?);
    if base == remote_tip {
        if has_unjournaled_local_changes(repo, &remote_tip)? {
            journal.state = PlanningSyncState::Pending;
            journal.detail = Some("local planning edits have not been journaled for sync".into());
            journal.write(state_root)?;
            return Ok(journal.status());
        }
        journal.confirmed_commit = Some(remote_tip);
        journal.confirmed_at_ms = Some(now_ms());
        journal.state = PlanningSyncState::Synced;
        journal.detail = None;
        journal.write(state_root)?;
        return Ok(journal.status());
    }
    if !git::is_ancestor(repo, &base, &remote_tip)? {
        journal.state = PlanningSyncState::Conflicted;
        journal.detail = Some("canonical planning history changed unexpectedly".into());
        journal.write(state_root)?;
        return Ok(journal.status());
    }
    let refresh = SyncOperation {
        operation_id: uuid::Uuid::nil(),
        workspace_id: journal.workspace_id,
        base_commit: base,
        canonical_ref: manifest.canonical_ref.clone(),
        message: String::new(),
        created_at_ms: now_ms(),
        remote_base_commit: Some(remote_tip.clone()),
        local_commit: None,
        mutations: Vec::new(),
    };
    match refresh_local_records(repo, &refresh, &remote_tip, &[]) {
        Ok(true) => {}
        Ok(false) => {
            journal.state = PlanningSyncState::Pending;
            journal.detail =
                Some("remote changes are waiting for local edits to be resolved".into());
            journal.write(state_root)?;
            return Ok(journal.status());
        }
        Err(error)
            if error
                .to_string()
                .contains("remote planning record is corrupt or incompatible") =>
        {
            return incompatible_remote(journal, state_root);
        }
        Err(error) => return Err(error),
    }
    if has_unjournaled_local_changes(repo, &remote_tip)? {
        journal.state = PlanningSyncState::Pending;
        journal.detail = Some("local planning edits have not been journaled for sync".into());
        journal.write(state_root)?;
        return Ok(journal.status());
    }
    journal.confirmed_commit = Some(remote_tip);
    journal.confirmed_at_ms = Some(now_ms());
    journal.state = PlanningSyncState::Synced;
    journal.detail = None;
    journal.write(state_root)?;
    Ok(journal.status())
}

pub(super) enum FetchError {
    Transport(anyhow::Error),
    Incompatible,
}

pub(super) fn fetch_verified(
    repo: &Path,
    expected: &WorkspaceManifest,
) -> Result<String, FetchError> {
    let tip = git::fetch(repo, &expected.canonical_ref).map_err(FetchError::Transport)?;
    verify_remote_manifest(repo, &tip, expected)?;
    Ok(tip)
}

pub(super) fn verify_remote_manifest(
    repo: &Path,
    commit: &str,
    expected: &WorkspaceManifest,
) -> Result<(), FetchError> {
    let bytes = git::read_path(repo, commit, "manifest.json")
        .map_err(|_| FetchError::Incompatible)?
        .ok_or(FetchError::Incompatible)?;
    let actual: WorkspaceManifest =
        serde_json::from_slice(&bytes).map_err(|_| FetchError::Incompatible)?;
    if actual.schema_version != 1
        || actual.workspace_id != expected.workspace_id
        || actual.kind != expected.kind
        || actual.canonical_ref != expected.canonical_ref
    {
        return Err(FetchError::Incompatible);
    }
    Ok(())
}

pub(super) fn incompatible_remote(
    journal: &mut OperationJournal,
    state_root: &Path,
) -> anyhow::Result<PlanningSyncStatus> {
    journal.state = PlanningSyncState::IncompatibleSchema;
    journal.detail = Some("canonical remote identity or schema is incompatible".into());
    journal.write(state_root)?;
    Ok(journal.status())
}

pub(super) fn has_unjournaled_local_changes(repo: &Path, remote_tip: &str) -> anyhow::Result<bool> {
    for path in git::worktree_paths(repo)? {
        if !sync_path(&path) {
            continue;
        }
        let local = intent::read_worktree_file(repo, &path)?;
        let remote = git::read_path(repo, remote_tip, &path)?;
        if local != remote {
            return Ok(true);
        }
    }
    Ok(false)
}

fn safe_target(root: &Path, target: &Path) -> anyhow::Result<bool> {
    let relative = target.strip_prefix(root)?;
    let mut cursor = root.to_path_buf();
    for component in relative.components() {
        let std::path::Component::Normal(part) = component else {
            return Ok(false);
        };
        cursor.push(part);
        if fs::symlink_metadata(&cursor).is_ok_and(|metadata| metadata.file_type().is_symlink()) {
            return Ok(false);
        }
    }
    Ok(true)
}

fn sync_path(path: &str) -> bool {
    let relative = Path::new(path);
    let safe = !path.is_empty()
        && relative
            .components()
            .all(|component| matches!(component, std::path::Component::Normal(_)));
    safe && (path.starts_with("state/") || path.starts_with("planning/"))
        && !matches!(
            path,
            "state/work.json"
                | "state/workflow.json"
                | "planning/open-items.md"
                | "planning/resolved-items.json"
        )
}

use std::{collections::BTreeMap, path::Path};

use super::super::{
    git,
    journal::{OperationJournal, RecordMutation, SyncOperation},
    merge,
    model::PlanningSyncState,
};
use super::refresh::{FetchError, fetch_verified, verify_remote_manifest};
mod paths;
mod state;
pub(super) use paths::read_worktree_file;
use paths::{is_legacy_authoritative_path, is_sync_path, read_intent_file, record_id_for_path};
pub(super) use state::{journal_lock, now_ms, shared_manifest};

pub(in crate::core::planning_sync) fn prepare_local_intent(
    repo: &Path,
    state_root: &Path,
    message: &str,
    paths: &[String],
) -> anyhow::Result<Option<uuid::Uuid>> {
    // Ordinary and legacy project repositories may contain planning data
    // without the managed workspace manifest; they remain local-only.
    if !repo.join("manifest.json").is_file() {
        return Ok(None);
    }
    // Managed workspace creation writes its manifest before the initial
    // commit. There is no shared history to journal until HEAD exists.
    if git::head(repo).is_err() {
        return Ok(None);
    }
    let Some((manifest, remote_configured)) = shared_manifest(repo)? else {
        return Ok(None);
    };
    if !remote_configured {
        let _lock = journal_lock(state_root, manifest.workspace_id)?;
        let journal = OperationJournal::load_or_new(
            state_root,
            manifest.workspace_id,
            &manifest.canonical_ref,
        )?;
        if journal.remote_identity.is_some()
            || journal.confirmed_commit.is_some()
            || journal.state == PlanningSyncState::Synced
            || !journal.operations.is_empty()
            || !journal.conflicts.is_empty()
        {
            anyhow::bail!(
                "planning remote is not configured; restore or explicitly reconfigure it before committing shared planning changes"
            );
        }
        return Ok(None);
    }
    anyhow::ensure!(
        git::is_canonical_branch(repo, &manifest.canonical_ref)?,
        "shared planning writes are paused because this checkout is not on {}; switch to the canonical planning branch before continuing",
        manifest.canonical_ref
    );
    let _lock = journal_lock(state_root, manifest.workspace_id)?;
    let mut journal =
        OperationJournal::load_or_new(state_root, manifest.workspace_id, &manifest.canonical_ref)?;
    let remote_identity = git::remote_identity(repo)?
        .ok_or_else(|| anyhow::anyhow!("planning remote is not configured"))?;
    if journal
        .remote_identity
        .as_deref()
        .is_some_and(|identity| identity != remote_identity.as_str())
    {
        journal.state = PlanningSyncState::Pending;
        journal.detail = Some(
            "planning remote changed; sync must confirm the destination before new shared edits are queued".into(),
        );
        journal.write(state_root)?;
        anyhow::bail!(
            "planning remote changed; sync must confirm the destination before new shared edits are queued"
        );
    }
    if journal.remote_identity.is_none() {
        journal.remote_identity = Some(remote_identity);
        journal.write(state_root)?;
    }
    let base_commit = match journal.confirmed_commit.clone() {
        Some(commit) => commit,
        None => initialize_confirmed_base(repo, state_root, &manifest, &mut journal)?,
    };
    let head = git::head(repo)?;
    let Some(mutation_base) = git::merge_base(repo, &base_commit, &head)? else {
        journal.state = PlanningSyncState::Conflicted;
        journal.detail = Some("local and canonical planning histories have no common base".into());
        journal.write(state_root)?;
        anyhow::bail!("local and canonical planning histories have no common base");
    };
    let mut intent_paths = BTreeMap::new();
    for path in git::changed_paths(repo, &mutation_base, &head)? {
        let committed = git::read_path(repo, &head, &path)?;
        let canonical = git::read_path(repo, &base_commit, &path)?;
        if committed == canonical || read_worktree_file(repo, &path)? == canonical {
            continue;
        }
        intent_paths.insert(path, true);
    }
    for path in paths {
        intent_paths.insert(path.clone(), false);
    }
    for path in intent_paths.keys() {
        if is_legacy_authoritative_path(path)
            && git::read_path(repo, &mutation_base, path)?
                != read_intent_file(repo, &head, path, intent_paths[path])?
        {
            journal.state = PlanningSyncState::IncompatibleSchema;
            journal.detail =
                Some("shared synchronization requires normalized per-record planning files".into());
            journal.write(state_root)?;
            anyhow::bail!(
                "shared planning requires normalized per-record files; cannot commit authoritative monolithic state files"
            );
        }
    }
    let mutations = intent_paths
        .iter()
        .map(|(path, from_head)| {
            mutation_from_worktree(
                repo,
                &mutation_base,
                &head,
                &journal.operations,
                path,
                *from_head,
            )
        })
        .collect::<Result<Vec<_>, _>>()?
        .into_iter()
        .flatten()
        .collect::<Vec<_>>();
    if mutations.is_empty() {
        return Ok(None);
    }
    let operation_id = uuid::Uuid::new_v4();
    journal.operations.push(SyncOperation {
        operation_id,
        workspace_id: manifest.workspace_id,
        base_commit,
        canonical_ref: manifest.canonical_ref,
        message: crate::error::redact_secrets(message),
        created_at_ms: now_ms(),
        remote_base_commit: None,
        local_commit: None,
        mutations,
    });
    journal.state = PlanningSyncState::Pending;
    journal.detail = None;
    journal.write(state_root)?;
    Ok(Some(operation_id))
}

fn initialize_confirmed_base(
    repo: &Path,
    state_root: &Path,
    manifest: &crate::core::managed_workspace::WorkspaceManifest,
    journal: &mut OperationJournal,
) -> anyhow::Result<String> {
    let base = match fetch_verified(repo, manifest) {
        Ok(commit) => commit,
        Err(FetchError::Transport(fetch_error)) => {
            let Some(commit) = git::remote_tracking_commit(repo, &manifest.canonical_ref)? else {
                return Err(fetch_error);
            };
            if let Err(FetchError::Incompatible) = verify_remote_manifest(repo, &commit, manifest) {
                return reject_incompatible_remote(journal, state_root);
            }
            commit
        }
        Err(FetchError::Incompatible) => {
            return reject_incompatible_remote(journal, state_root);
        }
    };
    journal.confirmed_commit = Some(base.clone());
    journal.confirmed_at_ms = Some(now_ms());
    journal.write(state_root)?;
    Ok(base)
}

fn reject_incompatible_remote(
    journal: &mut OperationJournal,
    state_root: &Path,
) -> anyhow::Result<String> {
    journal.state = PlanningSyncState::IncompatibleSchema;
    journal.detail = Some("canonical remote identity or schema is incompatible".into());
    journal.write(state_root)?;
    anyhow::bail!("canonical remote identity or schema is incompatible")
}

fn mutation_from_worktree(
    repo: &Path,
    base_commit: &str,
    head: &str,
    operations: &[SyncOperation],
    path: &str,
    from_head: bool,
) -> anyhow::Result<Option<RecordMutation>> {
    if !is_sync_path(path) {
        return Ok(None);
    }
    let record_id = record_id_for_path(path).unwrap_or_else(|| path.to_owned());
    let base = match operations.iter().rev().find_map(|operation| {
        operation
            .mutations
            .iter()
            .find(|mutation| mutation.path == path)
    }) {
        Some(previous) => previous
            .desired
            .as_ref()
            .map(|value| value.as_bytes().to_vec()),
        None => git::read_path(repo, base_commit, path)?,
    };
    let desired = read_intent_file(repo, head, path, from_head)?;
    if base == desired {
        return Ok(None);
    }
    anyhow::ensure!(
        base.as_ref().is_none_or(|bytes| bytes.len() <= 1_048_576)
            && desired
                .as_ref()
                .is_none_or(|bytes| bytes.len() <= 1_048_576),
        "planning record exceeds the one-megabyte sync limit"
    );
    let expected_revision = if merge::record_json_uid(path).is_some() {
        Some(match base.as_deref() {
            Some(bytes) => {
                let value: serde_json::Value = serde_json::from_slice(bytes)?;
                value
                    .get("revision")
                    .and_then(serde_json::Value::as_u64)
                    .ok_or_else(|| anyhow::anyhow!("record revision is missing or invalid"))?
            }
            None => 0,
        })
    } else {
        None
    };
    let mutation = RecordMutation {
        path: path.to_owned(),
        record_id,
        expected_revision,
        base: base
            .map(|bytes| String::from_utf8(bytes).map_err(anyhow::Error::from))
            .transpose()?,
        desired: desired
            .map(|bytes| String::from_utf8(bytes).map_err(anyhow::Error::from))
            .transpose()?,
    };
    merge::validate_mutation(&mutation)?;
    Ok(Some(mutation))
}

use std::path::Path;

use super::super::{git, journal::OperationJournal, model::PlanningSyncStatus};
use super::intent::{journal_lock, shared_manifest};

pub(crate) fn cancel_pending_operation_at(
    repo: &Path,
    state_root: &Path,
    operation_id: uuid::Uuid,
) -> anyhow::Result<PlanningSyncStatus> {
    let (manifest, remote_configured) = shared_manifest(repo)?
        .ok_or_else(|| anyhow::anyhow!("planning repository is not managed"))?;
    anyhow::ensure!(
        remote_configured,
        "planning repository has no shared remote"
    );
    let canonical_branch = manifest
        .canonical_ref
        .strip_prefix("refs/heads/")
        .unwrap_or(&manifest.canonical_ref);
    anyhow::ensure!(
        git::is_canonical_branch(repo, &manifest.canonical_ref)?,
        "planning checkout must be on canonical branch {canonical_branch} before cancelling pending work"
    );
    let _repository_lock = crate::core::gitops::lock_repository(repo)?;
    let _lock = journal_lock(state_root, manifest.workspace_id)?;
    let mut journal =
        OperationJournal::load_or_new(state_root, manifest.workspace_id, &manifest.canonical_ref)?;
    let operation = journal
        .operations
        .iter()
        .find(|operation| operation.operation_id == operation_id)
        .cloned()
        .ok_or_else(|| anyhow::anyhow!("pending planning operation no longer exists"))?;
    anyhow::ensure!(
        operation.local_commit.is_none() && !is_included_in_head(repo, &operation)?,
        "only an uncommitted local planning operation can be cancelled"
    );
    for mutation in &operation.mutations {
        let current = super::intent::read_worktree_file(repo, &mutation.path)?;
        anyhow::ensure!(
            current.as_deref() == mutation.desired.as_deref().map(str::as_bytes),
            "planning record {} changed after the operation was queued; it was preserved",
            mutation.path
        );
    }
    for mutation in &operation.mutations {
        git::write_worktree_path(
            repo,
            &mutation.path,
            mutation.base.as_deref().map(str::as_bytes),
        )?;
    }
    journal
        .operations
        .retain(|pending| pending.operation_id != operation_id);
    journal
        .conflicts
        .retain(|conflict| conflict.operation_id != operation_id);
    journal.state = if journal.operations.is_empty() {
        super::super::model::PlanningSyncState::Synced
    } else {
        super::super::model::PlanningSyncState::Pending
    };
    journal.detail = None;
    journal.write(state_root)?;
    Ok(journal.status())
}

fn is_included_in_head(
    repo: &Path,
    operation: &super::super::journal::SyncOperation,
) -> anyhow::Result<bool> {
    let head = git::head(repo)?;
    for mutation in &operation.mutations {
        let current = git::read_path(repo, &head, &mutation.path)?;
        if current.as_deref() == mutation.desired.as_deref().map(str::as_bytes)
            && current.as_deref() != mutation.base.as_deref().map(str::as_bytes)
        {
            return Ok(true);
        }
    }
    Ok(false)
}

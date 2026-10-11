use std::path::Path;

use super::super::{
    journal::OperationJournal,
    model::{PlanningSyncState, PlanningSyncStatus},
};
use super::{
    git,
    intent::{journal_lock, shared_manifest},
    transport_state,
};

pub(crate) fn status_at(repo: &Path, state_root: &Path) -> anyhow::Result<PlanningSyncStatus> {
    let Some((manifest, remote_configured)) = shared_manifest(repo)? else {
        return Ok(PlanningSyncStatus::local());
    };
    let journal =
        OperationJournal::load_or_new(state_root, manifest.workspace_id, &manifest.canonical_ref)?;
    if !remote_configured {
        return Ok(remote_not_configured_status(&journal).unwrap_or_else(PlanningSyncStatus::local));
    }
    let Some(remote_identity) = git::remote_identity(repo)? else {
        return Ok(PlanningSyncStatus::local());
    };
    let mut status = journal.status();
    if journal.remote_identity.is_none() && has_pending_remote_work(&journal) {
        status.state = PlanningSyncState::Pending;
        status.detail = Some(
            "pending planning operations have no recorded remote identity; explicitly confirm the planning destination before syncing".into(),
        );
        return Ok(status);
    }
    if !git::is_canonical_branch(repo, &manifest.canonical_ref)? {
        status.state = PlanningSyncState::Pending;
        status.detail = Some(format!(
            "planning checkout is not on {}; shared synchronization is paused until it is restored",
            manifest.canonical_ref
        ));
        return Ok(status);
    }
    if journal.remote_identity.as_deref() != Some(remote_identity.as_str())
        && (journal.remote_identity.is_some()
            || journal.confirmed_commit.is_some()
            || status.state == PlanningSyncState::Synced)
    {
        status.state = PlanningSyncState::Pending;
        status.confirmed_commit = None;
        status.confirmed_at_ms = None;
        status.detail =
            Some("planning remote changed; sync is required to confirm this destination".into());
    }
    Ok(status)
}

pub(crate) fn configure_remote(
    repo: &Path,
    state_root: &Path,
    remote: &str,
) -> anyhow::Result<PlanningSyncStatus> {
    let Some((manifest, _)) = shared_manifest(repo)? else {
        anyhow::bail!("planning repository is not managed");
    };
    let _repository_lock = crate::core::gitops::lock_repository(repo)?;
    let _lock = journal_lock(state_root, manifest.workspace_id)?;
    let mut journal =
        OperationJournal::load_or_new(state_root, manifest.workspace_id, &manifest.canonical_ref)?;
    if !git::is_canonical_branch(repo, &manifest.canonical_ref)? {
        journal.state = PlanningSyncState::Pending;
        journal.detail = Some(format!(
            "planning checkout is not on {}; shared synchronization is paused until it is restored",
            manifest.canonical_ref
        ));
        journal.write(state_root)?;
        return Ok(journal.status());
    }
    let current_identity = git::remote_identity(repo)?;
    let requested_identity = git::remote_identity_for(repo, remote);
    let destination_changes = match journal.remote_identity.as_deref() {
        Some(identity) => identity != requested_identity,
        None => current_identity.as_deref() != Some(requested_identity.as_str()),
    };
    if destination_changes && has_pending_remote_work(&journal) {
        anyhow::bail!(
            "planning remote cannot be changed while local sync operations or conflicts are pending; sync or resolve them before changing the destination"
        );
    }
    git::set_origin(repo, remote)?;
    let remote_identity = git::remote_identity(repo)?
        .ok_or_else(|| anyhow::anyhow!("planning remote is not configured"))?;
    if update_remote_identity(&mut journal, &remote_identity) {
        journal.write(state_root)?;
    }
    Ok(journal.status())
}

pub(super) fn bootstrap_empty_remote(
    repo: &Path,
    manifest: &crate::core::managed_workspace::WorkspaceManifest,
    journal: &mut OperationJournal,
    state_root: &Path,
) -> anyhow::Result<bool> {
    if journal.confirmed_commit.is_some() || !journal.operations.is_empty() {
        return Ok(true);
    }
    let heads = match git::remote_heads(repo) {
        Ok(heads) => heads,
        Err(error) => {
            let detail = error.to_string();
            journal.state = transport_state(&detail);
            journal.detail = Some(crate::error::redact_secrets(&detail));
            journal.write(state_root)?;
            return Ok(false);
        }
    };
    if heads.is_empty() {
        return match git::push_initial(repo, &manifest.canonical_ref) {
            Ok(()) => Ok(true),
            Err(error) => {
                let detail = error.to_string();
                journal.state = transport_state(&detail);
                journal.detail = Some(crate::error::redact_secrets(&detail));
                journal.write(state_root)?;
                Ok(false)
            }
        };
    }
    let canonical_exists = heads
        .iter()
        .any(|reference| reference == &manifest.canonical_ref);
    if !canonical_exists {
        journal.state = PlanningSyncState::IncompatibleSchema;
        journal.detail = Some(
            "planning remote already contains branches but has no canonical workspace branch; existing history was preserved".into(),
        );
        journal.write(state_root)?;
        return Ok(false);
    }
    Ok(true)
}

pub(super) fn update_remote_identity(
    journal: &mut OperationJournal,
    remote_identity: &str,
) -> bool {
    if journal.remote_identity.as_deref() == Some(remote_identity) {
        return false;
    }
    let needs_confirmation = journal.remote_identity.is_some()
        || journal.confirmed_commit.is_some()
        || journal.state == PlanningSyncState::Synced;
    journal.remote_identity = Some(remote_identity.to_owned());
    if needs_confirmation {
        journal.confirmed_commit = None;
        journal.confirmed_local_commit = None;
        journal.confirmed_at_ms = None;
        journal.state = PlanningSyncState::Pending;
        journal.detail =
            Some("planning remote changed; sync is required to confirm this destination".into());
    }
    true
}

pub(super) fn has_pending_remote_work(journal: &OperationJournal) -> bool {
    !journal.operations.is_empty() || !journal.conflicts.is_empty()
}

pub(super) fn remote_not_configured_status(
    journal: &OperationJournal,
) -> Option<PlanningSyncStatus> {
    if journal.remote_identity.is_none()
        && !has_pending_remote_work(journal)
        && matches!(
            journal.state,
            PlanningSyncState::Local | PlanningSyncState::Pending
        )
    {
        return None;
    }
    let mut status = journal.status();
    status.state = if journal.conflicts.is_empty() {
        PlanningSyncState::Pending
    } else {
        PlanningSyncState::Conflicted
    };
    status.detail = Some(if journal.operations.is_empty() {
        "planning remote is not configured; prior sync state is retained locally".into()
    } else {
        "planning remote is not configured; pending planning changes were preserved".into()
    });
    Some(status)
}

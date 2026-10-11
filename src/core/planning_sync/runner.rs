use std::path::Path;

mod intent;
mod reapply;
mod refresh;
pub(super) use intent::prepare_local_intent;
use intent::{journal_lock, now_ms, shared_manifest};
use reapply::record_local_commit;
pub(super) use reapply::{reapply, transport_state};
use refresh::{
    FetchError, fetch_verified, has_unjournaled_local_changes, incompatible_remote,
    refresh_local_records, refresh_workspace,
};

use super::{
    git,
    journal::OperationJournal,
    model::{PlanningSyncConflict, PlanningSyncState, PlanningSyncStatus},
};

const MAX_PUSH_ATTEMPTS: usize = 3;

pub(crate) fn commit_shared_intent(
    repo: &Path,
    message: &str,
    paths: &[String],
) -> Result<Option<uuid::Uuid>, crate::error::AppError> {
    prepare_local_intent(repo, &crate::persistence::state_root(), message, paths)
        .map_err(|error| crate::error::AppError::Other(error.to_string()))
}

pub(crate) fn finish_shared_intent(repo: &Path, operation_id: uuid::Uuid) {
    let root = crate::persistence::state_root();
    let _ = record_local_commit(repo, &root, operation_id);
    let _ = sync_with_state_root(repo, &root);
}

pub(crate) fn sync_workspace_at(
    repo: &Path,
    state_root: &Path,
) -> anyhow::Result<PlanningSyncStatus> {
    sync_with_state_root(repo, state_root)
}

pub(crate) fn status_at(repo: &Path, state_root: &Path) -> anyhow::Result<PlanningSyncStatus> {
    let Some((manifest, remote_configured)) = shared_manifest(repo)? else {
        return Ok(PlanningSyncStatus::local());
    };
    if !remote_configured {
        return Ok(PlanningSyncStatus::local());
    }
    let journal =
        OperationJournal::load_or_new(state_root, manifest.workspace_id, &manifest.canonical_ref)?;
    Ok(journal.status())
}

pub(super) fn sync_with_state_root(
    repo: &Path,
    state_root: &Path,
) -> anyhow::Result<PlanningSyncStatus> {
    let Some((manifest, remote_configured)) = shared_manifest(repo)? else {
        return Ok(PlanningSyncStatus::local());
    };
    if !remote_configured {
        return Ok(PlanningSyncStatus::local());
    }
    let _lock = journal_lock(state_root, manifest.workspace_id)?;
    let mut journal =
        OperationJournal::load_or_new(state_root, manifest.workspace_id, &manifest.canonical_ref)?;
    if journal.operations.is_empty() {
        return refresh_workspace(repo, state_root, &manifest, &mut journal);
    }

    while let Some(operation) = journal.operations.first().cloned() {
        let mut accepted = None;
        let mut failure_state = PlanningSyncState::Pending;
        let mut failure_detail = None;
        for _attempt in 0..MAX_PUSH_ATTEMPTS {
            let remote_base = match fetch_verified(repo, &manifest) {
                Ok(commit) => commit,
                Err(FetchError::Transport(error)) => {
                    failure_detail = Some(error.to_string());
                    failure_state = transport_state(&error.to_string());
                    break;
                }
                Err(FetchError::Incompatible) => {
                    return incompatible_remote(&mut journal, state_root);
                }
            };
            if let Some(pending) = journal.operations.first_mut()
                && pending.operation_id == operation.operation_id
            {
                pending.remote_base_commit = Some(remote_base.clone());
                journal.write(state_root)?;
            }
            if git::contains_operation(repo, &remote_base, operation.operation_id)? {
                accepted = Some(remote_base);
                break;
            }

            let (mutations, already_applied, conflicts) =
                match reapply(repo, &remote_base, &operation.mutations) {
                    Ok(result) => result,
                    Err(_) => {
                        journal.state = PlanningSyncState::IncompatibleSchema;
                        journal.detail =
                            Some("a remote planning record is corrupt or incompatible".into());
                        journal.write(state_root)?;
                        return Ok(journal.status());
                    }
                };
            if !conflicts.is_empty() {
                for conflict in conflicts {
                    let duplicate = journal.conflicts.iter().any(|existing| {
                        existing.operation_id == operation.operation_id
                            && existing.path == conflict.mutation.path
                    });
                    if !duplicate {
                        journal.conflicts.push(PlanningSyncConflict {
                            operation_id: operation.operation_id,
                            path: conflict.mutation.path.clone(),
                            record_id: conflict.mutation.record_id.clone(),
                            expected_revision: conflict.mutation.expected_revision,
                            base_commit: operation.base_commit.clone(),
                            remote_commit: remote_base.clone(),
                            fields: conflict.fields,
                            local_value: conflict.mutation.desired,
                            remote_value: conflict.remote_value,
                        });
                    }
                }
                journal.state = PlanningSyncState::Conflicted;
                journal.detail = Some("a shared planning record changed incompatibly".into());
                journal.write(state_root)?;
                return Ok(journal.status());
            }
            if already_applied {
                accepted = Some(remote_base);
                break;
            }
            let candidate = git::create_commit(
                repo,
                &remote_base,
                operation.operation_id,
                &operation.message,
                &mutations,
            )?;
            match git::push(repo, &candidate, &operation.canonical_ref) {
                Ok(()) => match fetch_verified(repo, &manifest) {
                    Ok(tip) if git::is_ancestor(repo, &candidate, &tip)? => {
                        accepted = Some(tip);
                        break;
                    }
                    Ok(tip) if git::contains_operation(repo, &tip, operation.operation_id)? => {
                        accepted = Some(tip);
                        break;
                    }
                    Ok(_) => {}
                    Err(FetchError::Transport(error)) => {
                        failure_detail = Some(error.to_string());
                        failure_state = transport_state(&error.to_string());
                        break;
                    }
                    Err(FetchError::Incompatible) => {
                        return incompatible_remote(&mut journal, state_root);
                    }
                },
                Err(error) => {
                    failure_detail = Some(error.to_string());
                    failure_state = transport_state(&error.to_string());
                    if failure_state != PlanningSyncState::Pending {
                        break;
                    }
                    match fetch_verified(repo, &manifest) {
                        Ok(tip) if git::contains_operation(repo, &tip, operation.operation_id)? => {
                            accepted = Some(tip);
                            break;
                        }
                        Err(FetchError::Incompatible) => {
                            return incompatible_remote(&mut journal, state_root);
                        }
                        Ok(_) | Err(FetchError::Transport(_)) => {}
                    }
                }
            }
        }

        if let Some(confirmed) = accepted {
            let later_operations = journal
                .operations
                .iter()
                .skip(1)
                .cloned()
                .collect::<Vec<_>>();
            match refresh_local_records(repo, &operation, &confirmed, &later_operations) {
                Ok(true) => {}
                Ok(false) => {
                    journal.state = PlanningSyncState::Pending;
                    journal.detail = Some(
                        "remote accepted the operation, but local edits prevented a safe refresh"
                            .into(),
                    );
                    journal.write(state_root)?;
                    return Ok(journal.status());
                }
                Err(error) if is_incompatible_record_error(&error) => {
                    return incompatible_remote(&mut journal, state_root);
                }
                Err(error) => return Err(error),
            }
            let local_edits_pending = has_unjournaled_local_changes(repo, &confirmed)?;
            journal.confirmed_commit = Some(confirmed);
            journal.confirmed_at_ms = Some(now_ms());
            journal.operations.remove(0);
            journal.state = if journal.operations.is_empty() && !local_edits_pending {
                PlanningSyncState::Synced
            } else {
                PlanningSyncState::Pending
            };
            journal.detail = local_edits_pending
                .then(|| "local planning edits have not been journaled for sync".into());
            journal.write(state_root)?;
            continue;
        }

        journal.state = failure_state;
        journal.detail =
            failure_detail.or_else(|| Some("remote rejected the operation after retrying".into()));
        journal.write(state_root)?;
        return Ok(journal.status());
    }
    Ok(journal.status())
}

fn is_incompatible_record_error(error: &anyhow::Error) -> bool {
    error
        .to_string()
        .contains("remote planning record is corrupt or incompatible")
}

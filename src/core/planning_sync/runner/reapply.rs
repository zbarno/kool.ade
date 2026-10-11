use std::path::Path;

use super::super::{
    git,
    journal::{OperationJournal, RecordMutation},
    merge::{self, MergeResult},
    model::PlanningSyncState,
};
use super::intent::{journal_lock, shared_manifest};

pub(in crate::core::planning_sync) fn reapply(
    repo: &Path,
    remote_base: &str,
    original: &[RecordMutation],
) -> anyhow::Result<(Vec<RecordMutation>, bool, Vec<ReapplyConflict>)> {
    let mut mutations = Vec::new();
    let mut conflicts = Vec::new();
    let mut already_applied = true;
    for mutation in original {
        let current = git::read_path(repo, remote_base, &mutation.path)?;
        match merge::apply_mutation(mutation, current.as_deref())? {
            MergeResult::Applied(desired) => {
                already_applied = false;
                mutations.push(RecordMutation {
                    desired: desired.map(String::from_utf8).transpose()?,
                    ..mutation.clone()
                });
            }
            MergeResult::AlreadyApplied => {}
            MergeResult::Conflict(fields) => conflicts.push(ReapplyConflict {
                mutation: mutation.clone(),
                remote_value: current.map(String::from_utf8).transpose()?,
                fields,
            }),
        }
    }
    Ok((mutations, already_applied, conflicts))
}

#[derive(Debug)]
pub(in crate::core::planning_sync) struct ReapplyConflict {
    pub(super) mutation: RecordMutation,
    pub(super) remote_value: Option<String>,
    pub(super) fields: Vec<String>,
}

pub(in crate::core::planning_sync) fn transport_state(detail: &str) -> PlanningSyncState {
    let detail = detail.to_ascii_lowercase();
    if detail.contains("credentials") {
        PlanningSyncState::AuthRequired
    } else if detail.contains("offline") {
        PlanningSyncState::Offline
    } else if detail.contains("write permission") {
        PlanningSyncState::ReadOnly
    } else {
        PlanningSyncState::Pending
    }
}

pub(super) fn record_local_commit(
    repo: &Path,
    state_root: &Path,
    operation_id: uuid::Uuid,
) -> anyhow::Result<()> {
    let Some((manifest, remote_configured)) = shared_manifest(repo)? else {
        return Ok(());
    };
    if !remote_configured {
        return Ok(());
    }
    let local_commit = git::head(repo)?;
    let _lock = journal_lock(state_root, manifest.workspace_id)?;
    let mut journal =
        OperationJournal::load_or_new(state_root, manifest.workspace_id, &manifest.canonical_ref)?;
    if let Some(operation) = journal
        .operations
        .iter_mut()
        .find(|operation| operation.operation_id == operation_id)
    {
        operation.local_commit = Some(local_commit);
        journal.write(state_root)?;
    }
    Ok(())
}

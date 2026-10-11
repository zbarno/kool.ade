mod cancel;
mod export;
mod intent;
mod reapply;
mod refresh;
mod remote;
mod resolve;
mod sync;
use super::git;
pub(crate) use cancel::cancel_pending_operation_at;
pub(crate) use export::export_pending_at;
pub(super) use intent::prepare_local_intent;
#[cfg(test)]
pub(super) use reapply::reapply;
use reapply::record_local_commit;
pub(super) use reapply::transport_state;
pub(crate) use remote::{configure_remote, status_at};
pub(crate) use resolve::resolve_conflict_at;
use std::path::Path;
use sync::sync_with_state_root_locked;

pub(crate) fn commit_shared_intent_at(
    repo: &Path,
    state_root: &Path,
    message: &str,
    paths: &[String],
) -> Result<Option<uuid::Uuid>, crate::error::AppError> {
    prepare_local_intent(repo, state_root, message, paths)
        .map_err(|error| crate::error::AppError::Other(error.to_string()))
}

#[cfg(test)]
pub(crate) fn finish_shared_intent_at(repo: &Path, state_root: &Path, operation_id: uuid::Uuid) {
    if let Ok(_repository_lock) = crate::core::gitops::lock_repository(repo) {
        finish_shared_intent_locked_at(repo, state_root, operation_id);
    }
}

pub(crate) fn finish_shared_intent_locked_at(
    repo: &Path,
    state_root: &Path,
    operation_id: uuid::Uuid,
) {
    let _ = record_local_commit(repo, state_root, operation_id);
    let _ = sync_with_state_root_locked(repo, state_root);
}

pub(crate) fn sync_workspace_at(
    repo: &Path,
    state_root: &Path,
) -> anyhow::Result<crate::core::planning_sync::PlanningSyncStatus> {
    sync_with_state_root(repo, state_root)
}

pub(super) fn sync_with_state_root(
    repo: &Path,
    state_root: &Path,
) -> anyhow::Result<crate::core::planning_sync::PlanningSyncStatus> {
    let _repository_lock = crate::core::gitops::lock_repository(repo)?;
    sync_with_state_root_locked(repo, state_root)
}

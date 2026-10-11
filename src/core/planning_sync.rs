//! Optimistic synchronization for managed planning Git repositories.
mod git;
mod journal;
mod merge;
mod model;
mod runner;

#[cfg(test)]
mod tests;

pub use model::{
    PlanningConflictChoice, PlanningSyncConflict, PlanningSyncMode, PlanningSyncPendingOperation,
    PlanningSyncState, PlanningSyncStatus,
};

pub(crate) fn remote_identity_for(repo: &std::path::Path, remote: &str) -> String {
    git::remote_identity_for(repo, remote)
}

pub(crate) use runner::cancel_pending_operation_at;
pub(crate) use runner::export_pending_at;
pub(crate) use runner::resolve_conflict_at;
pub(crate) use runner::{
    commit_shared_intent_at, configure_remote, finish_shared_intent_locked_at, sync_workspace_at,
};

pub(crate) use runner::status_at as sync_status_at;

pub(crate) fn validate_repository_config(repo: &std::path::Path) -> anyhow::Result<()> {
    git::validate_repository_config(repo)
}

pub(crate) fn validated_git_command(
    repo: &std::path::Path,
) -> anyhow::Result<std::process::Command> {
    git::validated_command(repo)
}

pub(crate) fn standalone_git_command() -> std::process::Command {
    git::standalone_command()
}

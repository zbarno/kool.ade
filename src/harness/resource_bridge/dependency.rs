mod lock_identity;
mod policy;
mod source;
#[cfg(test)]
mod tests;
mod triage;
mod validation;

pub(crate) use policy::decision_allowed;
pub(crate) use policy::manager_decision_allowed;
pub(super) use source::npm_registry_url;
pub(super) use triage::{from_unsupported_manager, triage};

pub(super) fn baseline_commit(worktree: &std::path::Path) -> Option<String> {
    lock_identity::baseline_commit(worktree)
}

pub(super) fn enrich_lock_identity(
    worktree: &std::path::Path,
    baseline_commit: Option<&str>,
    need: &mut crate::harness::DependencyNeed,
) -> anyhow::Result<()> {
    lock_identity::enrich(worktree, baseline_commit, need)
}

pub(super) fn validate_lockfile_identity(
    worktree: &std::path::Path,
    need: &crate::harness::DependencyNeed,
) -> anyhow::Result<()> {
    lock_identity::validate_current_identity(worktree, need)
}

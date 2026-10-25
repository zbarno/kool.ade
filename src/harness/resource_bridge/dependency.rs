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
pub(crate) use validation::system_install_packages;

pub(super) fn baseline_commit(worktree: &std::path::Path) -> Option<String> {
    lock_identity::baseline_commit(worktree)
}

#[cfg(test)]
pub(super) fn enrich_lock_identity(
    worktree: &std::path::Path,
    baseline_commit: Option<&str>,
    need: &mut crate::harness::DependencyNeed,
) -> anyhow::Result<()> {
    lock_identity::enrich(worktree, baseline_commit, need)
}

pub(super) fn enrich_lock_identity_scoped(
    worktree: &std::path::Path,
    dependency_root: &std::path::Path,
    dependency_relative_root: &str,
    baseline_commit: Option<&str>,
    need: &mut crate::harness::DependencyNeed,
) -> anyhow::Result<()> {
    lock_identity::enrich_scoped(
        worktree,
        dependency_root,
        dependency_relative_root,
        baseline_commit,
        need,
    )
}

pub(super) fn validate_lockfile_identity(
    worktree: &std::path::Path,
    need: &crate::harness::DependencyNeed,
) -> anyhow::Result<()> {
    lock_identity::validate_current_identity_at(worktree, need)
}

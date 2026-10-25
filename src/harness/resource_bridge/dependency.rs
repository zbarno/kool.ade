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
#[cfg(unix)]
pub(super) use triage::{from_unsupported_manager, triage};
pub(crate) use validation::system_install_packages;
pub(crate) use validation::valid_package_identity;

pub(crate) fn has_locked_package(
    root: &std::path::Path,
    ecosystem: crate::harness::PackageEcosystem,
    package: &str,
) -> anyhow::Result<bool> {
    let packages = match ecosystem {
        crate::harness::PackageEcosystem::Npm => super::npm::package_identities(root)?,
        crate::harness::PackageEcosystem::Cargo => {
            let contents = std::fs::read_to_string(root.join("Cargo.lock"))?;
            super::cargo::package_identities_from_contents(&contents)?
        }
        _ => anyhow::bail!("the selected ecosystem has no verified lockfile reader"),
    };
    Ok(packages.iter().any(|identity| identity.package == package))
}

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

use crate::harness::{DependencyKind, DependencyNeed, PackageEcosystem};
use sha2::{Digest, Sha256};
use std::{collections::BTreeSet, path::Path, process::Command};

const MAX_TREE_LIST_BYTES: usize = 32 * 1024 * 1024;
const MAX_BASE_LOCK_BYTES: usize = 32 * 1024 * 1024;
const MAX_BASE_MANIFEST_BYTES: usize = 2 * 1024 * 1024;
const MAX_BASE_TOTAL_LOCK_BYTES: usize = 128 * 1024 * 1024;
const MAX_LOCKFILES: usize = 128;

pub(super) fn baseline_commit(worktree: &Path) -> Option<String> {
    git_output(worktree, &["rev-parse", "--verify", "HEAD"])
        .ok()
        .and_then(|output| String::from_utf8(output).ok())
        .map(|commit| commit.trim().to_owned())
        .filter(|commit| !commit.is_empty())
}

pub(super) fn enrich(
    worktree: &Path,
    baseline_commit: Option<&str>,
    need: &mut DependencyNeed,
) -> anyhow::Result<()> {
    let supported = match need.ecosystem {
        PackageEcosystem::Npm => matches!(
            need.kind,
            DependencyKind::ExistingRestore
                | DependencyKind::NewProjectDependency
                | DependencyKind::DevelopmentDependency
        ),
        PackageEcosystem::Cargo => need.kind == DependencyKind::ExistingRestore,
        _ => false,
    };
    if !supported {
        return Ok(());
    }
    let current = current_packages(worktree, need.ecosystem)?;
    let current_manifests = if need.ecosystem == PackageEcosystem::Npm {
        Some(super::super::npm::dependency_manifest_inputs(worktree)?)
    } else {
        None
    };
    let baseline = match baseline_commit {
        Some(commit) => baseline_packages(worktree, commit, need.ecosystem)?,
        None => Vec::new(),
    };
    let baseline = baseline.into_iter().collect::<BTreeSet<_>>();
    let introduced = current
        .iter()
        .filter(|package| !baseline.contains(*package))
        .cloned()
        .collect::<Vec<_>>();
    let baseline_manifests = match (need.ecosystem, baseline_commit) {
        (PackageEcosystem::Npm, Some(commit)) => Some(baseline_manifest_inputs(worktree, commit)?),
        (PackageEcosystem::Npm, None) => Some(Vec::new()),
        _ => None,
    };
    let manifests_changed = current_manifests != baseline_manifests;
    if matches!(
        need.kind,
        DependencyKind::NewProjectDependency | DependencyKind::DevelopmentDependency
    ) && (!introduced.is_empty() || manifests_changed)
    {
        anyhow::bail!(
            "Npm dependency files changed before authorization. Submit the specific npm add command before editing dependency manifests or lockfiles; no packages were fetched."
        );
    }
    if need.kind == DependencyKind::ExistingRestore && manifests_changed && introduced.is_empty() {
        anyhow::bail!(
            "Npm dependency manifests changed without matching lockfile package identities. Submit a specific npm add command for the new dependency; no packages were fetched."
        );
    }
    need.lockfile_identity = Some(identity(&current, current_manifests.as_deref())?);
    need.introduced_packages = introduced;
    Ok(())
}

pub(super) fn validate_current_identity(
    worktree: &Path,
    need: &DependencyNeed,
) -> anyhow::Result<()> {
    let expected = need
        .lockfile_identity
        .as_deref()
        .ok_or_else(|| anyhow::anyhow!("The authorized lockfile identity is missing"))?;
    let packages = current_packages(worktree, need.ecosystem)?;
    let manifests = if need.ecosystem == PackageEcosystem::Npm {
        Some(super::super::npm::dependency_manifest_inputs(worktree)?)
    } else {
        None
    };
    anyhow::ensure!(
        identity(&packages, manifests.as_deref())? == expected,
        "The task dependency inputs changed after authorization"
    );
    Ok(())
}

fn current_packages(
    worktree: &Path,
    ecosystem: PackageEcosystem,
) -> anyhow::Result<Vec<crate::harness::DependencyPackageIdentity>> {
    match ecosystem {
        PackageEcosystem::Npm => super::super::npm::package_identities(worktree),
        PackageEcosystem::Cargo => super::super::cargo::package_identities_from_contents(
            &std::fs::read_to_string(worktree.join("Cargo.lock"))?,
        ),
        _ => anyhow::bail!("This package ecosystem has no lockfile identity reader"),
    }
}

fn baseline_packages(
    worktree: &Path,
    commit: &str,
    ecosystem: PackageEcosystem,
) -> anyhow::Result<Vec<crate::harness::DependencyPackageIdentity>> {
    let listing = git_output(
        worktree,
        &["ls-tree", "-r", "-z", "--name-only", "--full-tree", commit],
    )?;
    anyhow::ensure!(
        listing.len() <= MAX_TREE_LIST_BYTES,
        "Task baseline file list exceeds the lockfile scan limit"
    );
    let mut packages = BTreeSet::new();
    let mut files = 0;
    let mut total_bytes = 0_usize;
    for path in listing
        .split(|byte| *byte == 0)
        .filter(|path| !path.is_empty())
    {
        let path = std::str::from_utf8(path)?;
        if !is_lockfile(path, ecosystem) {
            continue;
        }
        files += 1;
        anyhow::ensure!(
            files <= MAX_LOCKFILES,
            "Task baseline contains too many lockfiles"
        );
        let object = format!("{commit}:{path}");
        let size = git_output(worktree, &["cat-file", "-s", &object])?;
        let size = std::str::from_utf8(&size)?.trim().parse::<usize>()?;
        anyhow::ensure!(
            size <= MAX_BASE_LOCK_BYTES,
            "Task baseline lockfile is too large"
        );
        total_bytes = total_bytes.saturating_add(size);
        anyhow::ensure!(
            total_bytes <= MAX_BASE_TOTAL_LOCK_BYTES,
            "Task baseline lockfiles exceed the scan budget"
        );
        let bytes = git_output(worktree, &["show", &object])?;
        anyhow::ensure!(
            bytes.len() == size,
            "Task baseline lockfile changed while scanning"
        );
        let entries = match ecosystem {
            PackageEcosystem::Npm => super::super::npm::packages_from_bytes(&bytes)?,
            PackageEcosystem::Cargo => {
                super::super::cargo::package_identities_from_contents(std::str::from_utf8(&bytes)?)?
            }
            _ => anyhow::bail!("This package ecosystem has no lockfile identity reader"),
        };
        packages.extend(entries);
    }
    Ok(packages.into_iter().collect())
}

fn baseline_manifest_inputs(
    worktree: &Path,
    commit: &str,
) -> anyhow::Result<Vec<(String, serde_json::Value)>> {
    let listing = git_output(
        worktree,
        &["ls-tree", "-r", "-z", "--name-only", "--full-tree", commit],
    )?;
    anyhow::ensure!(
        listing.len() <= MAX_TREE_LIST_BYTES,
        "Task baseline file list exceeds the npm manifest scan limit"
    );
    let mut inputs = Vec::new();
    let mut files = 0;
    let mut total_bytes = 0_usize;
    for path in listing
        .split(|byte| *byte == 0)
        .filter(|path| !path.is_empty())
    {
        let path = std::str::from_utf8(path)?;
        if !is_npm_package_manifest(path) {
            continue;
        }
        files += 1;
        anyhow::ensure!(
            files <= MAX_LOCKFILES * 2,
            "Task baseline contains too many npm manifests"
        );
        let object = format!("{commit}:{path}");
        let size = git_output(worktree, &["cat-file", "-s", &object])?;
        let size = std::str::from_utf8(&size)?.trim().parse::<usize>()?;
        anyhow::ensure!(
            size <= MAX_BASE_MANIFEST_BYTES,
            "Task baseline npm manifest is too large"
        );
        total_bytes = total_bytes.saturating_add(size);
        anyhow::ensure!(
            total_bytes <= MAX_BASE_TOTAL_LOCK_BYTES,
            "Task baseline npm manifests exceed the scan budget"
        );
        let bytes = git_output(worktree, &["show", &object])?;
        anyhow::ensure!(
            bytes.len() == size,
            "Task baseline npm manifest changed while scanning"
        );
        let input = super::super::npm::dependency_manifest_inputs_from_bytes(&bytes)?;
        if !input.as_object().is_some_and(serde_json::Map::is_empty) {
            inputs.push((path.to_owned(), input));
        }
    }
    inputs.sort_by(|left, right| left.0.cmp(&right.0));
    Ok(inputs)
}

fn is_npm_package_manifest(path: &str) -> bool {
    path.rsplit('/').next() == Some("package.json")
        && !path.split('/').any(|part| {
            matches!(
                part,
                ".git"
                    | "node_modules"
                    | ".koolade-packet"
                    | "target"
                    | "dist"
                    | "build"
                    | "coverage"
                    | "vendor"
                    | "obj"
                    | "bin"
            )
        })
}

fn identity(
    packages: &[crate::harness::DependencyPackageIdentity],
    npm_manifests: Option<&[(String, serde_json::Value)]>,
) -> anyhow::Result<String> {
    let bytes = serde_json::to_vec(&(packages, npm_manifests))?;
    Ok(format!("sha256:{:x}", Sha256::digest(bytes)))
}

fn is_lockfile(path: &str, ecosystem: PackageEcosystem) -> bool {
    if ecosystem == PackageEcosystem::Npm
        && path.split('/').any(|part| {
            matches!(
                part,
                ".git"
                    | "node_modules"
                    | ".koolade-packet"
                    | "target"
                    | "dist"
                    | "build"
                    | "coverage"
                    | "vendor"
                    | "obj"
                    | "bin"
            )
        })
    {
        return false;
    }
    match ecosystem {
        PackageEcosystem::Npm => matches!(
            path.rsplit('/').next(),
            Some("package-lock.json" | "npm-shrinkwrap.json")
        ),
        PackageEcosystem::Cargo => path == "Cargo.lock",
        _ => false,
    }
}

#[cfg(test)]
mod tests;

fn git_output(worktree: &Path, arguments: &[&str]) -> anyhow::Result<Vec<u8>> {
    let output = Command::new("git")
        .arg("-C")
        .arg(worktree)
        .args(arguments)
        .env_clear()
        .env("PATH", std::env::var_os("PATH").unwrap_or_default())
        .output()?;
    anyhow::ensure!(
        output.status.success(),
        "Could not inspect the task baseline lockfiles"
    );
    Ok(output.stdout)
}

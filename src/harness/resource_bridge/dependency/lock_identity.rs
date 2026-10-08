use crate::harness::{DependencyKind, DependencyNeed, PackageEcosystem};
use sha2::{Digest, Sha256};
use std::{collections::BTreeSet, path::Path, process::Command};

const MAX_TREE_LIST_BYTES: usize = 32 * 1024 * 1024;
const MAX_BASE_LOCK_BYTES: usize = 32 * 1024 * 1024;
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
    if need.kind != DependencyKind::ExistingRestore
        || !matches!(
            need.ecosystem,
            PackageEcosystem::Npm | PackageEcosystem::Cargo
        )
    {
        return Ok(());
    }
    let current = current_packages(worktree, need.ecosystem)?;
    let baseline = match baseline_commit {
        Some(commit) => baseline_packages(worktree, commit, need.ecosystem)?,
        None => Vec::new(),
    };
    let baseline = baseline.into_iter().collect::<BTreeSet<_>>();
    need.lockfile_identity = Some(identity(&current)?);
    need.introduced_packages = current
        .into_iter()
        .filter(|package| !baseline.contains(package))
        .collect();
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
    anyhow::ensure!(
        identity(&packages)? == expected,
        "The task lockfile package-set integrity identity changed after authorization"
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

fn identity(packages: &[crate::harness::DependencyPackageIdentity]) -> anyhow::Result<String> {
    let bytes = serde_json::to_vec(packages)?;
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

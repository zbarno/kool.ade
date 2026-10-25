//! Hash-locked Python wheel preparation through the mediated PyPI proxy.
use std::path::{Path, PathBuf};

mod adapter;
mod cache;
mod lockfile;
mod preparation;

pub(super) use adapter::Adapter;
pub(super) use preparation::SharedPreparationOperations;

pub(super) fn package_identities(
    root: &Path,
) -> anyhow::Result<Vec<crate::harness::DependencyPackageIdentity>> {
    lockfile::package_identities(root)
}

pub(super) fn package_identities_from_contents(
    contents: &str,
) -> anyhow::Result<Vec<crate::harness::DependencyPackageIdentity>> {
    lockfile::package_identities_from_contents(contents)
}

pub(super) fn persistent_cache_at(state_root: &Path) -> anyhow::Result<PathBuf> {
    cache::persistent_cache_at(state_root)
}

pub(super) fn snapshot_root(cache_root: &Path, lockfile_identity: &str) -> anyhow::Result<PathBuf> {
    cache::snapshot_root(cache_root, lockfile_identity)
}

#[cfg(test)]
pub(super) use preparation::test_preparation_operations;

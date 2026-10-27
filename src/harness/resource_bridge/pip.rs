//! Hash-locked Python wheel preparation through the mediated PyPI proxy.
use std::path::Path;
#[cfg(unix)]
use std::path::PathBuf;

#[cfg(unix)]
mod adapter;
#[cfg(unix)]
mod cache;
mod lockfile;
#[cfg(unix)]
mod preparation;

#[cfg(unix)]
pub(super) use adapter::Adapter;
#[cfg(unix)]
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

#[cfg(unix)]
pub(super) fn persistent_cache_at(state_root: &Path) -> anyhow::Result<PathBuf> {
    cache::persistent_cache_at(state_root)
}

#[cfg(unix)]
pub(super) fn snapshot_root(cache_root: &Path, lockfile_identity: &str) -> anyhow::Result<PathBuf> {
    cache::snapshot_root(cache_root, lockfile_identity)
}

#[cfg(all(test, unix))]
pub(super) use preparation::test_preparation_operations;

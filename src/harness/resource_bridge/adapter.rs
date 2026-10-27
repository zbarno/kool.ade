//! Shared contract and registry for package-manager acquisition adapters.
use crate::harness::{DependencyDecision, DependencyNeed, PackageEcosystem};
use std::{path::Path, sync::atomic::AtomicUsize};

use super::ResourceResponse;

pub(super) struct PreparationContext<'a> {
    pub(super) decision: DependencyDecision,
    pub(super) allow_downloads: bool,
    pub(super) worktree: &'a Path,
    pub(super) resource_dir: &'a Path,
    pub(super) npm_cache: &'a Path,
    pub(super) npm_snapshot: &'a Path,
    pub(super) cargo_cache: &'a Path,
    pub(super) pip_cache: &'a Path,
    pub(super) pip_snapshot: &'a Path,
    pub(super) go_cache: &'a Path,
    pub(super) go_snapshot: &'a Path,
    #[cfg(unix)]
    pub(super) go_operations: Option<&'a super::go::SharedPreparationOperations>,
    pub(super) downloaded_bytes: &'a AtomicUsize,
    pub(super) npm_operations: Option<&'a super::npm::SharedPreparationOperations>,
    pub(super) cargo_operations: Option<&'a super::cargo::SharedPreparationOperations>,
    #[cfg(unix)]
    pub(super) pip_operations: Option<&'a super::pip::SharedPreparationOperations>,
    pub(super) npm_cache_gate: &'a std::sync::Mutex<()>,
    pub(super) cargo_cache_gate: &'a std::sync::Mutex<()>,
    pub(super) pip_cache_gate: &'a std::sync::Mutex<()>,
    pub(super) go_cache_gate: &'a std::sync::Mutex<()>,
}

pub(super) trait DependencyAdapter: Sync {
    fn ecosystem(&self) -> PackageEcosystem;
    fn ecosystem_label(&self) -> &'static str;
    fn prepare(
        &self,
        context: &PreparationContext<'_>,
        need: &DependencyNeed,
    ) -> anyhow::Result<ResourceResponse>;
}

#[cfg(unix)]
static NPM: super::npm::Adapter = super::npm::Adapter;
#[cfg(unix)]
static CARGO: super::cargo::Adapter = super::cargo::Adapter;
#[cfg(unix)]
static PIP: super::pip::Adapter = super::pip::Adapter;
#[cfg(unix)]
static GO: super::go::Adapter = super::go::Adapter;

#[cfg(unix)]
pub(super) fn for_ecosystem(ecosystem: PackageEcosystem) -> Option<&'static dyn DependencyAdapter> {
    let adapter: &'static dyn DependencyAdapter = match ecosystem {
        PackageEcosystem::Npm => &NPM,
        PackageEcosystem::Cargo => &CARGO,
        PackageEcosystem::Pip => &PIP,
        PackageEcosystem::Go => &GO,
        _ => return None,
    };
    Some(adapter)
}

#[cfg(all(test, unix))]
mod tests {
    use super::for_ecosystem;
    use crate::harness::PackageEcosystem;

    #[test]
    fn registry_has_distinct_npm_cargo_pip_and_go_adapters_and_fails_closed_elsewhere() {
        assert_eq!(
            for_ecosystem(PackageEcosystem::Npm).map(|adapter| adapter.ecosystem()),
            Some(PackageEcosystem::Npm)
        );
        assert_eq!(
            for_ecosystem(PackageEcosystem::Cargo).map(|adapter| adapter.ecosystem()),
            Some(PackageEcosystem::Cargo)
        );
        assert_eq!(
            for_ecosystem(PackageEcosystem::Pip).map(|adapter| adapter.ecosystem()),
            Some(PackageEcosystem::Pip)
        );
        assert_eq!(
            for_ecosystem(PackageEcosystem::Go).map(|adapter| adapter.ecosystem()),
            Some(PackageEcosystem::Go)
        );
        assert!(for_ecosystem(PackageEcosystem::Nuget).is_none());
        assert!(for_ecosystem(PackageEcosystem::Other).is_none());
    }
}

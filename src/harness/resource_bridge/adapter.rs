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
    pub(super) downloaded_bytes: &'a AtomicUsize,
    pub(super) npm_operations: Option<&'a super::npm::SharedPreparationOperations>,
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

static NPM: super::npm::Adapter = super::npm::Adapter;
static CARGO: super::cargo::Adapter = super::cargo::Adapter;

pub(super) fn for_ecosystem(ecosystem: PackageEcosystem) -> Option<&'static dyn DependencyAdapter> {
    let adapter: &'static dyn DependencyAdapter = match ecosystem {
        PackageEcosystem::Npm => &NPM,
        PackageEcosystem::Cargo => &CARGO,
        _ => return None,
    };
    Some(adapter)
}

#[cfg(test)]
mod tests {
    use super::for_ecosystem;
    use crate::harness::PackageEcosystem;

    #[test]
    fn registry_has_distinct_npm_and_cargo_adapters_and_fails_closed_elsewhere() {
        assert_eq!(
            for_ecosystem(PackageEcosystem::Npm).map(|adapter| adapter.ecosystem()),
            Some(PackageEcosystem::Npm)
        );
        assert_eq!(
            for_ecosystem(PackageEcosystem::Cargo).map(|adapter| adapter.ecosystem()),
            Some(PackageEcosystem::Cargo)
        );
        assert!(for_ecosystem(PackageEcosystem::Nuget).is_none());
        assert!(for_ecosystem(PackageEcosystem::Other).is_none());
    }
}

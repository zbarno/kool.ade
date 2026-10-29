use crate::harness::resource_bridge::adapter::{DependencyAdapter, PreparationContext};

pub(in crate::harness::resource_bridge) struct Adapter;

impl DependencyAdapter for Adapter {
    fn ecosystem(&self) -> crate::harness::PackageEcosystem {
        crate::harness::PackageEcosystem::Pip
    }

    fn ecosystem_label(&self) -> &'static str {
        "pip"
    }

    fn prepare(
        &self,
        context: &PreparationContext<'_>,
        need: &crate::harness::DependencyNeed,
    ) -> anyhow::Result<crate::harness::resource_bridge::ResourceResponse> {
        anyhow::ensure!(
            crate::harness::dependency_decision_allowed(need, context.decision),
            "Pip dependency request did not pass its exact authorization decision"
        );
        anyhow::ensure!(
            need.kind == crate::harness::DependencyKind::ExistingRestore
                && need.package.is_none()
                && need.version.is_none(),
            "Pip supports hash-locked requirements.txt restores only"
        );
        let _cache_gate = context
            .pip_cache_gate
            .lock()
            .map_err(|_| anyhow::anyhow!("Pip cache coordination lock is unavailable"))?;
        crate::harness::resource_bridge::dependency::validate_lockfile_identity(
            context.worktree,
            need,
        )?;
        super::preparation::prepare_with_download_policy_and_operations(
            context.worktree,
            context.pip_cache,
            context.pip_snapshot,
            context.downloaded_bytes,
            context.allow_downloads,
            context.pip_operations,
            need.lockfile_identity.as_deref().ok_or_else(|| {
                anyhow::anyhow!("The authorized requirements.txt identity is missing")
            })?,
        )
    }
}

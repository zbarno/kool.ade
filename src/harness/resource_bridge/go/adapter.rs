use crate::harness::resource_bridge::ResourceResponse;

pub(in crate::harness::resource_bridge) struct Adapter;

impl super::super::adapter::DependencyAdapter for Adapter {
    fn ecosystem(&self) -> crate::harness::PackageEcosystem {
        crate::harness::PackageEcosystem::Go
    }

    fn ecosystem_label(&self) -> &'static str {
        "Go modules"
    }

    fn prepare(
        &self,
        context: &super::super::adapter::PreparationContext<'_>,
        need: &crate::harness::DependencyNeed,
    ) -> anyhow::Result<ResourceResponse> {
        anyhow::ensure!(
            crate::harness::dependency_decision_allowed(need, context.decision),
            "Go module request did not pass its exact authorization decision"
        );
        anyhow::ensure!(
            need.kind == crate::harness::DependencyKind::ExistingRestore,
            "Go currently supports exact go.mod/go.sum existing restores only"
        );
        let _cache_gate = context
            .go_cache_gate
            .lock()
            .map_err(|_| anyhow::anyhow!("Go cache coordination lock is unavailable"))?;
        crate::harness::resource_bridge::dependency::validate_lockfile_identity(
            context.worktree,
            need,
        )?;
        super::prepare::prepare_with_download_policy(
            context.worktree,
            context.go_cache,
            context.go_snapshot,
            context.downloaded_bytes,
            context.allow_downloads,
            context.go_operations,
            need.lockfile_identity
                .as_deref()
                .ok_or_else(|| anyhow::anyhow!("Go lockfile identity is missing"))?,
        )
    }
}

use super::*;
use crate::harness::resource_bridge::adapter::{DependencyAdapter, PreparationContext};

pub(in crate::harness::resource_bridge) struct Adapter;

impl DependencyAdapter for Adapter {
    fn ecosystem(&self) -> crate::harness::PackageEcosystem {
        crate::harness::PackageEcosystem::Npm
    }

    fn ecosystem_label(&self) -> &'static str {
        "npm"
    }

    fn prepare(
        &self,
        context: &PreparationContext<'_>,
        need: &crate::harness::DependencyNeed,
    ) -> anyhow::Result<ResourceResponse> {
        anyhow::ensure!(
            crate::harness::dependency_decision_allowed(need, context.decision),
            "npm dependency request did not pass its exact authorization decision"
        );
        match need.kind {
            crate::harness::DependencyKind::ExistingRestore => {
                crate::harness::resource_bridge::dependency::validate_lockfile_identity(
                    context.worktree,
                    need,
                )?;
                match (need.package.as_deref(), need.version.as_deref()) {
                    (Some(package), Some(version)) => anyhow::ensure!(
                        verified_lock_identity(context.worktree, package, version),
                        "The npm package identity is not checksum-backed by the project lockfile"
                    ),
                    (None, None) => {}
                    _ => anyhow::bail!(
                        "An npm lockfile restore must provide both package name and exact version, or neither"
                    ),
                }
                let registry = crate::harness::resource_bridge::dependency::npm_registry_url(
                    need.source.as_deref(),
                )
                .ok_or_else(|| anyhow::anyhow!("The authorized npm registry source is invalid"))?;
                match context.npm_operations {
                    Some(operations) => prepare_with_registry_and_ops(
                        PreparationRequest {
                            worktree: context.worktree,
                            response_dir: context.resource_dir,
                            npm_cache: context.npm_cache,
                            npm_snapshot: context.npm_snapshot,
                            purpose: &need.reason,
                            downloaded_bytes: context.downloaded_bytes,
                            authorized_registry: Some(&registry),
                        },
                        PreparationOperations {
                            retrieve: operations.retrieve.as_ref(),
                            index_cache: operations.index_cache.as_ref(),
                        },
                    ),
                    None => prepare_with_registry(
                        context.worktree,
                        context.resource_dir,
                        context.npm_cache,
                        context.npm_snapshot,
                        &need.reason,
                        context.downloaded_bytes,
                        Some(&registry),
                    ),
                }
            }
            crate::harness::DependencyKind::NewProjectDependency
            | crate::harness::DependencyKind::DevelopmentDependency => match context.npm_operations
            {
                Some(operations) => addition::prepare_with_operations(
                    addition::AdditionRequest {
                        worktree: context.worktree,
                        response_dir: context.resource_dir,
                        npm_cache: context.npm_cache,
                        npm_snapshot: context.npm_snapshot,
                        need,
                        decision: context.decision,
                        downloaded_bytes: context.downloaded_bytes,
                    },
                    operations,
                ),
                None => prepare_addition(
                    context.worktree,
                    context.resource_dir,
                    context.npm_cache,
                    context.npm_snapshot,
                    need,
                    context.decision,
                    context.downloaded_bytes,
                ),
            },
            crate::harness::DependencyKind::SystemTool => {
                anyhow::bail!("npm cannot prepare a system tool dependency")
            }
        }
    }
}

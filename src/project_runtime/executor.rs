use std::{collections::BTreeMap, path::Path};

use crate::{
    execution::session::ExecutionSession,
    project_runtime::{
        AppSurface, ProjectRuntimeStore, RuntimeOperation, RuntimeReceiptState, RuntimeReceiptV1,
    },
};

mod artifacts;
mod provenance;
mod start;

use artifacts::{artifact_capture_paths, receipt_artifact_hashes};

/// Resolves only persisted, approved target recipes and records their observed outcome.
pub struct ProjectRuntimeExecutor<'store, 'planning> {
    store: &'store ProjectRuntimeStore<'planning>,
}

impl<'store, 'planning> ProjectRuntimeExecutor<'store, 'planning> {
    pub fn new(store: &'store ProjectRuntimeStore<'planning>) -> Self {
        Self { store }
    }

    pub fn execute(
        &self,
        session: &ExecutionSession,
        target_id: &str,
        operation: RuntimeOperation,
    ) -> anyhow::Result<Vec<RuntimeReceiptV1>> {
        let (manifest, target) = self.validate_candidate(session, target_id, operation)?;
        if operation == RuntimeOperation::Start {
            return start::execute(self.store, session, &manifest, &target);
        }
        let recipe_ids = match operation {
            RuntimeOperation::Build => target.build_recipe_ref.iter().collect::<Vec<_>>(),
            RuntimeOperation::Test => target.test_recipe_refs.iter().collect::<Vec<_>>(),
            RuntimeOperation::Start => unreachable!("start dispatch returned above"),
        };
        let mut receipts = Vec::with_capacity(recipe_ids.len());
        let workspace = session.workspace();
        for recipe_id in recipe_ids {
            let recipe = manifest
                .recipes
                .iter()
                .chain(manifest.user_overrides.iter())
                .find(|recipe| recipe.recipe_id == recipe_id.as_str())
                .ok_or_else(|| anyhow::anyhow!("runtime target recipe is missing"))?;
            let capture_paths = artifact_capture_paths(&manifest, &target, recipe, operation)?;
            let mut recipe = recipe.clone();
            recipe.working_directory_relative = join_workspace_path(
                &target.project_relative_root,
                &recipe.working_directory_relative,
            )?;
            recipe.allowed_output_paths = recipe
                .allowed_output_paths
                .iter()
                .map(|path| join_workspace_path(&target.project_relative_root, path))
                .collect::<anyhow::Result<_>>()?;
            recipe.expected_artifacts = recipe
                .expected_artifacts
                .iter()
                .map(|path| join_workspace_path(&target.project_relative_root, path))
                .collect::<anyhow::Result<_>>()?;
            let output = match session.run_scoped_recipe_with_artifacts(
                self.store,
                &target.target_id,
                &recipe,
                &target.project_relative_root,
                &capture_paths,
            ) {
                Ok(output) => output,
                Err(error) => {
                    let receipt = RuntimeReceiptV1 {
                        schema_version: 1,
                        receipt_id: uuid::Uuid::new_v4().hyphenated().to_string(),
                        operation,
                        task_id: workspace.task_id.clone(),
                        target_id: target.target_id.clone(),
                        source_sha: workspace.source_sha.clone(),
                        recipe_id: Some(recipe.recipe_id),
                        state: RuntimeReceiptState::Failed,
                        exit_code: None,
                        artifact_hashes: BTreeMap::new(),
                        log_reference: None,
                        observed_at_unix_ms: next_receipt_timestamp(
                            self.store,
                            &workspace.task_id,
                        )?,
                    };
                    self.store.append_receipt(&receipt)?;
                    return Err(anyhow::Error::new(error));
                }
            };
            let artifact_hashes =
                receipt_artifact_hashes(&target.project_relative_root, &output.result.artifacts)?;
            let expected_artifacts_present = recipe.expected_artifacts.iter().all(|expected| {
                let scoped_expected = expected
                    .strip_prefix(&format!("{}/", target.project_relative_root))
                    .unwrap_or(expected);
                output
                    .result
                    .artifacts
                    .iter()
                    .any(|artifact| artifact.relative_path == scoped_expected)
            });
            let passed = output
                .result
                .exit_code
                .is_some_and(|code| recipe.expected_exit_codes.contains(&code))
                && !output.result.cancelled
                && !output.result.timed_out
                && expected_artifacts_present;
            let receipt = RuntimeReceiptV1 {
                schema_version: 1,
                receipt_id: uuid::Uuid::new_v4().hyphenated().to_string(),
                operation,
                task_id: workspace.task_id.clone(),
                target_id: target.target_id.clone(),
                source_sha: workspace.source_sha.clone(),
                recipe_id: Some(recipe.recipe_id),
                state: if passed {
                    RuntimeReceiptState::Passed
                } else {
                    RuntimeReceiptState::Failed
                },
                exit_code: output.result.exit_code,
                artifact_hashes,
                log_reference: Some(output.result.execution_id),
                observed_at_unix_ms: next_receipt_timestamp(self.store, &workspace.task_id)?,
            };
            self.store.append_receipt(&receipt)?;
            receipts.push(receipt);
        }
        Ok(receipts)
    }
}

fn validate_manifest_candidate(
    manifest: &crate::project_runtime::ProjectCapabilityManifestV1,
    workspace: &crate::execution::contract::WorkspaceRef,
    target_id: &str,
    operation: RuntimeOperation,
    allow_verified_adapter: bool,
) -> anyhow::Result<crate::project_runtime::AppTargetV1> {
    anyhow::ensure!(
        manifest.workspace_id == workspace.project_id
            && manifest.source_sha == workspace.source_sha,
        "runtime manifest does not match the pinned task workspace"
    );
    let target = manifest
        .candidate_targets
        .iter()
        .find(|target| target.target_id == target_id)
        .ok_or_else(|| anyhow::anyhow!("runtime target is not in the approved manifest"))?;
    anyhow::ensure!(
        target.owning_task_id == workspace.task_id,
        "runtime target belongs to a different task"
    );
    anyhow::ensure!(
        target.repository_id == manifest.repository_id,
        "runtime target belongs to a different repository"
    );
    anyhow::ensure!(
        manifest.is_active_target(&workspace.task_id, target_id),
        "runtime target is not explicitly selected for this task"
    );
    anyhow::ensure!(
        target.app_surface != AppSurface::UnsupportedTarget,
        "runtime target is unsupported on this platform"
    );
    let recipe_ids = match operation {
        RuntimeOperation::Build => target.build_recipe_ref.iter().collect::<Vec<_>>(),
        RuntimeOperation::Test => target.test_recipe_refs.iter().collect::<Vec<_>>(),
        RuntimeOperation::Start => target.start_recipe_ref.iter().collect::<Vec<_>>(),
    };
    anyhow::ensure!(
        !recipe_ids.is_empty(),
        "runtime target has no recipe for this operation"
    );
    for recipe_id in recipe_ids {
        let recipe = manifest
            .recipes
            .iter()
            .chain(manifest.user_overrides.iter())
            .find(|recipe| recipe.recipe_id == recipe_id.as_str())
            .ok_or_else(|| anyhow::anyhow!("runtime target recipe is missing"))?;
        anyhow::ensure!(
            recipe.is_explicitly_user_approved()
                || (allow_verified_adapter
                    && recipe.source == crate::project_runtime::RecipeSource::TrustedAdapter),
            "project metadata recipes require explicit approval or verified adapter provenance before execution"
        );
    }
    Ok(target.clone())
}

pub(super) fn join_workspace_path(root: &str, working_directory: &str) -> anyhow::Result<String> {
    crate::project_runtime::recipe::validate_relative_path(root).map_err(anyhow::Error::msg)?;
    crate::project_runtime::recipe::validate_relative_path(working_directory)
        .map_err(anyhow::Error::msg)?;
    let joined = Path::new(root).join(working_directory);
    let normalized = joined
        .components()
        .filter_map(|component| match component {
            std::path::Component::CurDir => None,
            std::path::Component::Normal(part) => Some(part.to_string_lossy().into_owned()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("/");
    Ok(if normalized.is_empty() {
        ".".into()
    } else {
        normalized
    })
}

fn now_unix_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .min(u64::MAX as u128) as u64
}

fn next_receipt_timestamp(store: &ProjectRuntimeStore<'_>, task_id: &str) -> anyhow::Result<u64> {
    let latest = store
        .receipts_for_task(task_id)?
        .iter()
        .map(|receipt| receipt.observed_at_unix_ms)
        .max()
        .unwrap_or_default();
    Ok(next_timestamp(now_unix_ms(), latest))
}

fn next_timestamp(now: u64, latest: u64) -> u64 {
    now.max(latest.saturating_add(1))
}

#[cfg(test)]
#[path = "executor/tests.rs"]
mod tests;

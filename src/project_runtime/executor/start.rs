use std::{collections::BTreeMap, time::Duration};

mod artifacts;
use artifacts::{ReceiptData, build_artifacts, failed_start, receipt};
mod readiness;
use readiness::start_script;

#[cfg(test)]
pub(crate) fn qa_go_start_script(
    start: &crate::project_runtime::ExecutionRecipeV1,
    probe: &crate::project_runtime::ReadinessProbeV1,
) -> anyhow::Result<String> {
    readiness::qa_go_start_script(start, probe)
}
#[cfg(test)]
use readiness::{readiness_command, readiness_setup_command};
mod receipts;

use crate::{
    execution::session::ExecutionSession,
    project_runtime::{
        ProjectCapabilityManifestV1, ProjectRuntimeStore, RuntimeReceiptV1, TargetCapabilityState,
    },
};

const MAX_START_SECONDS: u64 = 120;

pub(super) fn execute(
    store: &ProjectRuntimeStore<'_>,
    session: &ExecutionSession,
    manifest: &ProjectCapabilityManifestV1,
    target: &super::super::AppTargetV1,
) -> anyhow::Result<Vec<RuntimeReceiptV1>> {
    let recipe_id = target
        .start_recipe_ref
        .as_deref()
        .ok_or_else(|| anyhow::anyhow!("runtime target has no start recipe"))?;
    let probe_id = target
        .readiness_probe_ref
        .as_deref()
        .ok_or_else(|| anyhow::anyhow!("runtime target has no readiness probe"))?;
    let recipe = manifest
        .recipes
        .iter()
        .chain(manifest.user_overrides.iter())
        .find(|recipe| recipe.recipe_id.as_str() == recipe_id)
        .ok_or_else(|| anyhow::anyhow!("runtime start recipe is missing"))?;
    anyhow::ensure!(
        store.recipe_is_locally_approved(manifest, target, recipe),
        "project metadata recipes require explicit approval before execution"
    );
    let probe = manifest
        .readiness_probes
        .get(probe_id)
        .ok_or_else(|| anyhow::anyhow!("runtime readiness probe is missing"))?;
    probe
        .validate()
        .map_err(|_| anyhow::anyhow!("runtime readiness probe failed validation"))?;
    if let crate::project_runtime::ReadinessProbeV1::CustomApprovedProbe { recipe_id } = probe {
        let probe_recipe = manifest
            .recipes
            .iter()
            .chain(manifest.user_overrides.iter())
            .find(|candidate| candidate.recipe_id == *recipe_id)
            .ok_or_else(|| anyhow::anyhow!("custom readiness recipe is missing"))?;
        anyhow::ensure!(
            store.recipe_is_locally_approved(manifest, target, probe_recipe),
            "custom readiness recipe requires a local approval record"
        );
    }
    let receipts = store.receipts_for_task(&session.workspace().task_id)?;
    receipts::require_passed_test_receipts(target, &receipts, &session.workspace().source_sha)?;
    let Some(build_artifacts) =
        build_artifacts(manifest, target, &receipts, &session.workspace().source_sha)?
    else {
        return failed_start(store, session, target, recipe.recipe_id.clone());
    };
    require_startable_state(&target.capability_state)?;
    let artifact_paths = build_artifacts.paths;
    let build_hashes = build_artifacts.hashes;
    validate_start_executable_artifact(recipe, &artifact_paths)?;
    let script = start_script(manifest, target, recipe, probe)?;
    let source_input_paths = super::source_input_paths(
        manifest,
        target,
        crate::project_runtime::RuntimeOperation::Start,
        recipe.recipe_id.as_str(),
    )?;
    let source_output_paths =
        super::source_output_paths(manifest, target, recipe.recipe_id.as_str())?;
    let authorization_ref = session.verify_local_recipe_approval(
        store,
        &target.target_id,
        &scoped_start_recipe(target, recipe)?,
        Some(&target.project_relative_root),
    )?;
    let timeout = Duration::from_secs(recipe.timeout_seconds.min(MAX_START_SECONDS));
    let (output, process) = match session.start_scoped_runtime_process_with_artifacts(
        &script,
        timeout,
        &target.project_relative_root,
        authorization_ref,
        crate::execution::session::ScopedRuntimePaths {
            artifact_paths: &artifact_paths,
            source_output_paths: &source_output_paths,
            source_input_paths: &source_input_paths,
        },
    ) {
        Ok(result) => result,
        Err(_) => {
            let receipt = receipt(
                session,
                target,
                ReceiptData {
                    recipe_id: recipe.recipe_id.clone(),
                    exit_code: None,
                    log_reference: None,
                    artifact_hashes: BTreeMap::new(),
                    passed: false,
                    observed_at_unix_ms: super::next_receipt_timestamp(
                        store,
                        &session.workspace().task_id,
                    )?,
                },
            );
            store.append_receipt(&receipt)?;
            return Ok(vec![receipt]);
        }
    };
    let observed_hashes = output
        .result
        .artifacts
        .iter()
        .map(|artifact| {
            Ok((
                super::join_workspace_path(&target.project_relative_root, &artifact.relative_path)?,
                artifact.sha256.clone(),
            ))
        })
        .collect::<anyhow::Result<BTreeMap<_, _>>>()?;
    let passed = process.is_some()
        && output.result.exit_code == Some(0)
        && !output.result.cancelled
        && !output.result.timed_out
        && output.result.artifacts.len() == artifact_paths.len()
        && observed_hashes == build_hashes;
    let receipt = receipt(
        session,
        target,
        ReceiptData {
            recipe_id: recipe.recipe_id.clone(),
            exit_code: output.result.exit_code,
            log_reference: Some(output.result.execution_id),
            artifact_hashes: observed_hashes,
            passed,
            observed_at_unix_ms: super::next_receipt_timestamp(
                store,
                &session.workspace().task_id,
            )?,
        },
    );
    if !passed {
        if let Some(process) = process {
            let _ = process.stop();
        }
        store.append_receipt(&receipt)?;
        return Ok(vec![receipt]);
    }
    if let Err(error) = store.append_receipt(&receipt) {
        if let Some(process) = process {
            let _ = process.stop();
        }
        return Err(error.into());
    }
    Ok(vec![receipt])
}

fn scoped_start_recipe(
    target: &super::super::AppTargetV1,
    recipe: &crate::project_runtime::ExecutionRecipeV1,
) -> anyhow::Result<crate::project_runtime::ExecutionRecipeV1> {
    let root = &target.project_relative_root;
    let mut scoped = recipe.clone();
    scoped.working_directory_relative =
        super::join_workspace_path(root, &recipe.working_directory_relative)?;
    scoped.allowed_output_paths = recipe
        .allowed_output_paths
        .iter()
        .map(|path| super::join_workspace_path(root, path))
        .collect::<anyhow::Result<_>>()?;
    scoped.expected_artifacts = recipe
        .expected_artifacts
        .iter()
        .map(|path| super::join_workspace_path(root, path))
        .collect::<anyhow::Result<_>>()?;
    Ok(scoped)
}

fn validate_start_executable_artifact(
    recipe: &crate::project_runtime::ExecutionRecipeV1,
    artifact_paths: &[String],
) -> anyhow::Result<()> {
    if !crate::project_runtime::is_target_relative_executable_path(&recipe.executable_id) {
        return Ok(());
    }
    anyhow::ensure!(
        recipe.source == crate::project_runtime::RecipeSource::UserApproved,
        "target-relative Start executables require an explicit user-approved recipe"
    );
    let executable_path =
        target_relative_executable_path(&recipe.working_directory_relative, &recipe.executable_id)?;
    anyhow::ensure!(
        artifact_paths.iter().any(|path| path == &executable_path),
        "Start executable path must be included in the exact Build receipt artifact set"
    );
    Ok(())
}

fn target_relative_executable_path(directory: &str, executable: &str) -> anyhow::Result<String> {
    let path = if directory == "." {
        std::path::PathBuf::from(executable)
    } else {
        std::path::Path::new(directory).join(executable)
    };
    let components = path
        .components()
        .map(|component| match component {
            std::path::Component::Normal(part) => part
                .to_str()
                .ok_or_else(|| anyhow::anyhow!("Start executable path is not UTF-8")),
            _ => anyhow::bail!("Start executable path is not canonical and relative"),
        })
        .collect::<anyhow::Result<Vec<_>>>()?;
    anyhow::ensure!(!components.is_empty(), "Start executable path is empty");
    Ok(components.join("/"))
}

fn require_startable_state(state: &TargetCapabilityState) -> anyhow::Result<()> {
    // Capability states are discovery-time hints. A successful exact-source
    // Build, passed Test receipts, and the connected session's prerequisite
    // preflight prove that these recoverable gaps have been resolved.
    anyhow::ensure!(
        matches!(
            state,
            TargetCapabilityState::Ready
                | TargetCapabilityState::ReadyButUntested
                | TargetCapabilityState::MissingToolchain
                | TargetCapabilityState::MissingDependencies
                | TargetCapabilityState::DriverUnavailable
        ),
        "runtime target is not startable after prerequisite recovery"
    );
    Ok(())
}

#[cfg(test)]
#[path = "start/tests.rs"]
mod tests;

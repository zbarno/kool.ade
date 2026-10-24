use crate::project_runtime::{AppTargetV1, ProjectRuntimeStore, RuntimeOperation};

pub fn verify_prepared_candidate(
    runtime: &ProjectRuntimeStore<'_>,
    manifest: &crate::project_runtime::ProjectCapabilityManifestV1,
    request: &crate::runtime_qa::QaRunRequestV1,
    prepared: super::PreparedCandidate<'_>,
) -> Result<(), String> {
    if manifest.workspace_id != prepared.workspace.project_id
        || manifest.source_sha != prepared.workspace.source_sha
    {
        return Err(
            "project runtime manifest changed workspace or source before QA dispatch".into(),
        );
    }
    let planning_item_uid = request
        .planning_item_uid
        .as_deref()
        .ok_or_else(|| "prepared Runtime QA request lost its planning item identity".to_owned())?;
    if request.parent_task_uid != prepared.workspace.task_id
        || request.code_head_sha != prepared.workspace.source_sha
        || request.target_id.as_deref() != Some(prepared.target.target_id.as_str())
        || planning_item_uid != prepared.target.owning_task_id
    {
        return Err("prepared Runtime QA request no longer matches its task and target".into());
    }
    if !manifest.is_active_target(planning_item_uid, &prepared.target.target_id) {
        return Err("approved runtime target is no longer the active selection".into());
    }
    let current_target = manifest
        .candidate_targets
        .iter()
        .find(|target| {
            target.target_id == prepared.target.target_id
                && target.owning_task_id == planning_item_uid
        })
        .ok_or_else(|| "approved runtime target disappeared before QA dispatch".to_owned())?;
    if current_target != prepared.target {
        return Err("approved runtime target changed after QA request preparation".into());
    }
    verify_provenance_for_request(runtime, manifest, current_target)?;
    let current_readiness_probe = current_target
        .readiness_probe_ref
        .as_ref()
        .and_then(|reference| manifest.readiness_probes.get(reference));
    if current_readiness_probe != prepared.readiness_probe {
        return Err("approved readiness probe changed after QA request preparation".into());
    }
    let current_artifact_paths = current_target
        .artifact_spec_ref
        .as_ref()
        .and_then(|reference| manifest.artifact_specs.get(reference))
        .ok_or_else(|| "approved artifact allowlist disappeared before QA dispatch".to_owned())?
        .iter()
        .map(|path| super::workspace_target_path(&current_target.project_relative_root, path))
        .collect::<Result<Vec<_>, _>>()?;
    if current_artifact_paths != prepared.artifact_paths {
        return Err("approved artifact allowlist changed after QA request preparation".into());
    }
    let recipe = manifest
        .recipes
        .iter()
        .chain(manifest.user_overrides.iter())
        .find(|recipe| {
            Some(recipe.recipe_id.as_str()) == current_target.start_recipe_ref.as_deref()
        })
        .ok_or_else(|| "approved startup recipe disappeared before QA dispatch".to_owned())?;
    let mut current_startup_recipe = recipe.clone();
    current_startup_recipe.working_directory_relative = super::workspace_target_path(
        &current_target.project_relative_root,
        &recipe.working_directory_relative,
    )?;
    if current_startup_recipe != *prepared.startup_recipe {
        return Err("approved startup recipe changed after QA request preparation".into());
    }
    Ok(())
}

pub fn verify_provenance_for_request(
    runtime: &ProjectRuntimeStore<'_>,
    manifest: &crate::project_runtime::ProjectCapabilityManifestV1,
    target: &AppTargetV1,
) -> Result<(), String> {
    for operation in [RuntimeOperation::Build, RuntimeOperation::Start] {
        runtime
            .verify_execution_provenance(manifest, target, operation)
            .map_err(|error| {
                format!("runtime QA {operation:?} recipe provenance is invalid: {error}")
            })?;
    }
    Ok(())
}

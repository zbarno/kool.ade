use crate::project_runtime::{AppTargetV1, ExecutionRecipeV1, ReadinessProbeV1, RuntimeOperation};

use super::super::approval;

#[allow(clippy::too_many_arguments)]
pub(super) fn user_recipe_approval_sha256(
    workspace_id: &str,
    repository_id: &str,
    source_sha: &str,
    target: &AppTargetV1,
    operation: RuntimeOperation,
    recipe: &ExecutionRecipeV1,
    readiness: Option<&ReadinessProbeV1>,
    readiness_recipe: Option<&ExecutionRecipeV1>,
    artifact_spec: Option<&Vec<String>>,
) -> Result<String, String> {
    let mut stable_target = target.clone();
    stable_target.build_recipe_ref = None;
    stable_target.test_recipe_refs.clear();
    stable_target.start_recipe_ref = None;
    stable_target.readiness_probe_ref = None;
    stable_target.port_needs.clear();
    stable_target.capability_state =
        crate::project_runtime::TargetCapabilityState::ReadyButUntested;
    approval::digest_payload(&(
        workspace_id,
        repository_id,
        source_sha,
        stable_target,
        operation,
        recipe,
        readiness,
        readiness_recipe,
        artifact_spec,
    ))
}

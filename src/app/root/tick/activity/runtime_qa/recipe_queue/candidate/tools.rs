use std::collections::BTreeSet;

use crate::project_runtime::{
    AppTargetV1, ExecutionRecipeV1, ProjectCapabilityManifestV1, ReadinessProbeV1, RuntimeOperation,
};

pub(super) fn required_tools(
    manifest: &ProjectCapabilityManifestV1,
    target: &AppTargetV1,
    operation: RuntimeOperation,
    recipes: &[&ExecutionRecipeV1],
) -> Result<Option<Vec<String>>, String> {
    let mut selected = recipes.to_vec();
    if operation == RuntimeOperation::Start {
        let probe_id = target
            .readiness_probe_ref
            .as_deref()
            .ok_or("approved runtime target has no readiness probe")?;
        let probe = manifest
            .readiness_probes
            .get(probe_id)
            .ok_or("runtime readiness probe is missing")?;
        if let ReadinessProbeV1::CustomApprovedProbe { recipe_id } = probe {
            let recipe = manifest
                .recipes
                .iter()
                .chain(manifest.user_overrides.iter())
                .find(|recipe| recipe.recipe_id == *recipe_id)
                .ok_or("custom readiness recipe is missing")?;
            if !recipe.is_explicitly_user_approved() {
                return Ok(None);
            }
            selected.push(recipe);
        }
    }

    Ok(Some(
        selected
            .iter()
            .flat_map(|recipe| {
                std::iter::once(recipe.executable_id.as_str())
                    .chain(recipe.prerequisite_tool_ids.iter().map(String::as_str))
            })
            .map(str::to_owned)
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect(),
    ))
}

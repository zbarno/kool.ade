use std::collections::BTreeSet;

use crate::project_runtime::{
    AppTargetV1, ProjectCapabilityManifestV1, ProjectRuntimeStore, RuntimeOperation,
};

use super::join_workspace_path;

/// Existing reviewed-checkout paths accepted while this recipe stages in.
/// Includes its declared outputs and those from recipes that ran before it.
pub(super) fn source_input_paths(
    manifest: &ProjectCapabilityManifestV1,
    target: &AppTargetV1,
    operation: RuntimeOperation,
    current_recipe_id: &str,
) -> anyhow::Result<Vec<String>> {
    let mut recipe_ids = Vec::new();
    let mut append_recipe = |recipe_id: &str| {
        if !recipe_ids.iter().any(|seen| seen == recipe_id) {
            recipe_ids.push(recipe_id.to_owned());
        }
    };
    match operation {
        RuntimeOperation::Build => {
            let build_id = target
                .build_recipe_ref
                .as_deref()
                .ok_or_else(|| anyhow::anyhow!("runtime target build recipe is missing"))?;
            anyhow::ensure!(
                build_id == current_recipe_id,
                "runtime build input recipe does not match the target"
            );
            append_recipe(build_id);
        }
        RuntimeOperation::Test => {
            if let Some(build_id) = target.build_recipe_ref.as_deref() {
                append_recipe(build_id);
            }
            let mut found_current = false;
            for recipe_id in &target.test_recipe_refs {
                append_recipe(recipe_id);
                if recipe_id == current_recipe_id {
                    found_current = true;
                    break;
                }
            }
            anyhow::ensure!(
                found_current,
                "runtime test input recipe does not match the target"
            );
        }
        RuntimeOperation::Start => {
            if let Some(build_id) = target.build_recipe_ref.as_deref() {
                append_recipe(build_id);
            }
            for recipe_id in &target.test_recipe_refs {
                append_recipe(recipe_id);
            }
            let start_id = target
                .start_recipe_ref
                .as_deref()
                .ok_or_else(|| anyhow::anyhow!("runtime target start recipe is missing"))?;
            anyhow::ensure!(
                start_id == current_recipe_id,
                "runtime start input recipe does not match the target"
            );
            append_recipe(start_id);
        }
    }

    let recipes = recipe_ids
        .into_iter()
        .map(|recipe_id| {
            manifest
                .recipes
                .iter()
                .chain(manifest.user_overrides.iter())
                .find(|recipe| recipe.recipe_id == recipe_id)
                .ok_or_else(|| anyhow::anyhow!("runtime target output recipe is missing"))
        })
        .collect::<anyhow::Result<Vec<_>>>()?;
    let mut paths = BTreeSet::new();
    for path in recipes.iter().flat_map(|recipe| {
        recipe
            .allowed_output_paths
            .iter()
            .chain(recipe.expected_artifacts.iter())
    }) {
        paths.insert(join_workspace_path(&target.project_relative_root, path)?);
    }
    if let Some(artifact_spec) = target
        .artifact_spec_ref
        .as_ref()
        .and_then(|reference| manifest.artifact_specs.get(reference))
    {
        for path in artifact_spec {
            paths.insert(join_workspace_path(&target.project_relative_root, path)?);
        }
    }
    Ok(paths.into_iter().collect())
}

/// The current recipe's exact stage-out allowlist.
pub(super) fn source_output_paths(
    manifest: &ProjectCapabilityManifestV1,
    target: &AppTargetV1,
    recipe_id: &str,
) -> anyhow::Result<Vec<String>> {
    let recipe = manifest
        .recipes
        .iter()
        .chain(manifest.user_overrides.iter())
        .find(|recipe| recipe.recipe_id == recipe_id)
        .ok_or_else(|| anyhow::anyhow!("runtime target output recipe is missing"))?;
    let mut paths = BTreeSet::new();
    for path in recipe
        .allowed_output_paths
        .iter()
        .chain(recipe.expected_artifacts.iter())
    {
        paths.insert(join_workspace_path(&target.project_relative_root, path)?);
    }
    Ok(paths.into_iter().collect())
}

pub(super) fn validate_source_output_approvals(
    store: &ProjectRuntimeStore<'_>,
    manifest: &ProjectCapabilityManifestV1,
    target: &AppTargetV1,
    operation: RuntimeOperation,
) -> anyhow::Result<()> {
    let recipe_ids = match operation {
        RuntimeOperation::Build => target
            .build_recipe_ref
            .iter()
            .map(String::as_str)
            .collect::<Vec<_>>(),
        RuntimeOperation::Test => target
            .build_recipe_ref
            .iter()
            .map(String::as_str)
            .chain(target.test_recipe_refs.iter().map(String::as_str))
            .collect(),
        RuntimeOperation::Start => target
            .build_recipe_ref
            .iter()
            .map(String::as_str)
            .chain(target.test_recipe_refs.iter().map(String::as_str))
            .chain(target.start_recipe_ref.iter().map(String::as_str))
            .collect(),
    };
    for recipe_id in recipe_ids {
        let recipe = manifest
            .recipes
            .iter()
            .chain(manifest.user_overrides.iter())
            .find(|recipe| recipe.recipe_id == recipe_id)
            .ok_or_else(|| anyhow::anyhow!("runtime target output recipe is missing"))?;
        anyhow::ensure!(
            store.recipe_is_locally_approved(manifest, target, recipe),
            "runtime target output recipe is not locally approved"
        );
    }
    Ok(())
}

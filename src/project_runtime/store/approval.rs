use sha2::Digest;

use crate::artifacts::planning_store::StoreError;

use super::ProjectRuntimeStore;
use crate::project_runtime::{AppTargetV1, ExecutionRecipeV1, ProjectCapabilityManifestV1};

impl ProjectRuntimeStore<'_> {
    /// Checks the app-owned, per-project approval record for the exact recipe
    /// and target identity. The shared manifest marker alone is not authority.
    pub fn recipe_is_locally_approved(
        &self,
        manifest: &ProjectCapabilityManifestV1,
        target: &AppTargetV1,
        recipe: &ExecutionRecipeV1,
    ) -> bool {
        if recipe.source == crate::project_runtime::RecipeSource::TrustedAdapter {
            let stored_recipe = manifest
                .recipes
                .iter()
                .chain(manifest.user_overrides.iter())
                .find(|candidate| candidate.recipe_id == recipe.recipe_id);
            if stored_recipe != Some(recipe) {
                return false;
            }
            return recipe_operation(target, recipe).is_some_and(|operation| {
                self.verify_execution_provenance(manifest, target, operation)
                    .is_ok()
            });
        }
        if !recipe.is_explicitly_user_approved() {
            return false;
        }
        if let Some(operation) = recipe_operation(target, recipe)
            && recipe
                .evidence_refs
                .iter()
                .any(|reference| reference.starts_with("user-recipe-approval:"))
        {
            let custom_target = target.target_id.starts_with("user-target-")
                || Self::custom_target_origin_sha256(manifest, target)
                    .ok()
                    .is_some_and(|origin| {
                        crate::persistence::runtime_target_approvals::has_custom_target_origin(
                            &manifest.workspace_id,
                            &target.owning_task_id,
                            &origin,
                        )
                        .unwrap_or(false)
                    });
            if custom_target && self.verify_user_target_approval(manifest, target).is_err() {
                return false;
            }
            if self
                .verify_user_recipe_approval(manifest, target, operation, recipe)
                .is_ok()
            {
                return true;
            }
            return false;
        }
        let Ok((scope, digest)) = recipe_approval_fingerprints(manifest, target, recipe) else {
            return false;
        };
        crate::persistence::feature_approvals::contains_scoped(
            &self.store.git_root(),
            &scope,
            &digest,
        )
    }
}

fn recipe_operation(
    target: &AppTargetV1,
    recipe: &ExecutionRecipeV1,
) -> Option<crate::project_runtime::RuntimeOperation> {
    use crate::project_runtime::RuntimeOperation;

    let mut operations = [
        (target.build_recipe_ref.as_deref() == Some(recipe.recipe_id.as_str()))
            .then_some(RuntimeOperation::Build),
        target
            .test_recipe_refs
            .iter()
            .any(|recipe_id| recipe_id == &recipe.recipe_id)
            .then_some(RuntimeOperation::Test),
        (target.start_recipe_ref.as_deref() == Some(recipe.recipe_id.as_str()))
            .then_some(RuntimeOperation::Start),
    ]
    .into_iter()
    .flatten();
    let operation = operations.next()?;
    operations.next().is_none().then_some(operation)
}

fn recipe_approval_fingerprints(
    manifest: &ProjectCapabilityManifestV1,
    target: &AppTargetV1,
    recipe: &ExecutionRecipeV1,
) -> Result<(String, String), StoreError> {
    let scope = serde_json::to_vec(&(
        manifest.workspace_id.as_str(),
        manifest.repository_id.as_str(),
        manifest.source_sha.as_str(),
        target.target_id.as_str(),
        target.owning_task_id.as_str(),
        target.project_relative_root.as_str(),
        recipe.recipe_id.as_str(),
    ))
    .map_err(|error| StoreError::MalformedState(error.to_string()))?;
    let recipe_bytes = serde_json::to_vec(recipe)
        .map_err(|error| StoreError::MalformedState(error.to_string()))?;
    let scope = format!("{:x}", sha2::Sha256::digest(scope));
    let digest = format!("{:x}", sha2::Sha256::digest(recipe_bytes));
    Ok((scope, digest))
}

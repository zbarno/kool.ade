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
        if !recipe.is_explicitly_user_approved() {
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

    pub(super) fn record_recipe_approval(
        &self,
        manifest: &ProjectCapabilityManifestV1,
        target: &AppTargetV1,
        recipe: &ExecutionRecipeV1,
    ) -> Result<(), StoreError> {
        let (scope, digest) = recipe_approval_fingerprints(manifest, target, recipe)?;
        crate::persistence::feature_approvals::record_scoped(
            &self.store.git_root(),
            &scope,
            &digest,
        )
        .map_err(|error| {
            StoreError::MalformedState(format!("local recipe approval could not be saved: {error}"))
        })
    }
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

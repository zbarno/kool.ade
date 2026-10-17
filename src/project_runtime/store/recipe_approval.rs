use crate::artifacts::planning_store::StoreError;

use super::super::ProjectRuntimeStore;
use crate::project_runtime::{
    ExecutionRecipeV1, ProjectCapabilityManifestV1, ReadinessProbeV1, RecipeSource,
    RuntimeOperation,
};

impl<'a> ProjectRuntimeStore<'a> {
    /// Applies a user-approved recipe to a stored target under the same
    /// revision fence as discovery and saves it atomically.
    pub fn approve_recipe_and_save(
        &self,
        target_id: &str,
        operation: RuntimeOperation,
        recipe: ExecutionRecipeV1,
        approval_proof: crate::ui::layout::RuntimeTargetApprovalProof,
        expected_manifest_revision: u64,
    ) -> Result<ProjectCapabilityManifestV1, StoreError> {
        self.approve_recipe_with_readiness_and_save(
            target_id,
            operation,
            recipe,
            None,
            approval_proof,
            expected_manifest_revision,
        )
    }

    /// Saves a user-approved recipe and its optional readiness probe as one
    /// revision-fenced manifest update.
    pub fn approve_recipe_with_readiness_and_save(
        &self,
        target_id: &str,
        operation: RuntimeOperation,
        mut recipe: ExecutionRecipeV1,
        readiness: Option<ReadinessProbeV1>,
        approval_proof: crate::ui::layout::RuntimeTargetApprovalProof,
        expected_manifest_revision: u64,
    ) -> Result<ProjectCapabilityManifestV1, StoreError> {
        let next_revision = expected_manifest_revision.checked_add(1).ok_or_else(|| {
            StoreError::MalformedState("project runtime manifest revision is exhausted".into())
        })?;
        let mut manifest = self.load_manifest()?.ok_or_else(|| {
            StoreError::MalformedState("project runtime manifest is missing".into())
        })?;
        let existing_target = manifest
            .candidate_targets
            .iter()
            .find(|target| target.target_id == target_id)
            .ok_or_else(|| {
                StoreError::MalformedState("selected runtime target is missing".into())
            })?;
        let custom_origin = Self::custom_target_origin_sha256(&manifest, existing_target)
            .map_err(StoreError::MalformedState)?;
        if target_id.starts_with("user-target-")
            || crate::persistence::runtime_target_approvals::has_custom_target_origin(
                &manifest.workspace_id,
                &existing_target.owning_task_id,
                &custom_origin,
            )
            .map_err(|error| StoreError::MalformedState(error.to_string()))?
        {
            return Err(StoreError::MalformedState(
                "custom targets must be approved with their complete recipe set".into(),
            ));
        }
        if recipe.source != RecipeSource::UserApproved {
            return Err(StoreError::MalformedState(
                "only an explicitly user-approved recipe can be added here".into(),
            ));
        }
        let receipt_id = approval_proof.receipt_uid();
        if !recipe
            .evidence_refs
            .iter()
            .any(|reference| reference == &format!("user-approval:{receipt_id}"))
        {
            return Err(StoreError::MalformedState(
                "recipe approval proof does not match its user approval reference".into(),
            ));
        }
        recipe
            .evidence_refs
            .push(format!("user-recipe-approval:{receipt_id}"));
        let recipe_id = recipe.recipe_id.clone();
        crate::project_runtime::apply_user_recipe_override(
            &mut manifest,
            target_id,
            operation,
            recipe,
        )
        .map_err(StoreError::MalformedState)?;
        if let Some(probe) = readiness {
            probe.validate().map_err(StoreError::MalformedState)?;
            let probe_id = format!("readiness-{}", uuid::Uuid::new_v4().simple());
            let target = manifest
                .candidate_targets
                .iter_mut()
                .find(|target| target.target_id == target_id)
                .ok_or_else(|| {
                    StoreError::MalformedState("selected runtime target is missing".into())
                })?;
            if let ReadinessProbeV1::HttpOnApprovedPort { port, .. }
            | ReadinessProbeV1::LocalServiceHealth { port, .. } = &probe
            {
                target.port_needs = vec![*port];
            }
            target.readiness_probe_ref = Some(probe_id.clone());
            manifest.readiness_probes.insert(probe_id, probe);
        }
        let target = manifest
            .candidate_targets
            .iter()
            .find(|target| target.target_id == target_id)
            .ok_or_else(|| {
                StoreError::MalformedState("selected runtime target is missing".into())
            })?;
        let recipe = manifest
            .user_overrides
            .iter()
            .find(|recipe| recipe.recipe_id == recipe_id)
            .ok_or_else(|| StoreError::MalformedState("approved recipe is missing".into()))?;
        let binding = Self::user_recipe_approval_binding(&manifest, target, operation, recipe)
            .map_err(StoreError::MalformedState)?;
        crate::persistence::runtime_target_approvals::save(&binding).map_err(|error| {
            StoreError::MalformedState(format!(
                "operator-local recipe approval could not be saved: {error}"
            ))
        })?;
        // Persist the operator-local authority before publishing the shared
        // recipe. A failed manifest CAS can leave an inert binding, but cannot
        // leave a shared recipe that lacks its required local approval.
        self.save_manifest(&manifest, expected_manifest_revision)?;
        manifest.revision = next_revision;
        Ok(manifest)
    }
}

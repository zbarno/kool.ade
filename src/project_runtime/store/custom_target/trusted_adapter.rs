use crate::{
    artifacts::planning_store::StoreError,
    project_runtime::{
        ExecutionRecipeV1, ProjectCapabilityManifestV1, RecipeSource, RuntimeOperation,
    },
};

use super::super::ProjectRuntimeStore;

impl<'a> ProjectRuntimeStore<'a> {
    /// Persists a recipe emitted by trusted adapter code for a target whose
    /// discovery origin was recorded locally. The exact resulting candidate
    /// receives a separate operator-local provenance fingerprint.
    pub(crate) fn add_trusted_adapter_recipe_and_save(
        &self,
        target_id: &str,
        operation: RuntimeOperation,
        recipe: ExecutionRecipeV1,
        expected_manifest_revision: u64,
    ) -> Result<ProjectCapabilityManifestV1, StoreError> {
        recipe.validate().map_err(StoreError::MalformedState)?;
        if recipe.source != RecipeSource::TrustedAdapter {
            return Err(StoreError::MalformedState(
                "trusted adapter recipe has an invalid source".into(),
            ));
        }
        let mut manifest = self.load_manifest()?.ok_or_else(|| {
            StoreError::MalformedState("project runtime manifest is missing".into())
        })?;
        let target_index = manifest
            .candidate_targets
            .iter()
            .position(|target| target.target_id == target_id)
            .ok_or_else(|| StoreError::MalformedState("runtime target is missing".into()))?;
        let before = manifest.candidate_targets[target_index].clone();
        let discovered_sha = ProjectRuntimeStore::discovered_target_origin_sha256(
            &manifest.workspace_id,
            &manifest.source_sha,
            &before,
        )
        .map_err(StoreError::MalformedState)?;
        if !crate::persistence::runtime_target_approvals::has_discovered_target_origin(
            &manifest.workspace_id,
            &before.owning_task_id,
            &discovered_sha,
        )
        .map_err(|error| StoreError::MalformedState(error.to_string()))?
        {
            return Err(StoreError::MalformedState(
                "trusted adapter target is not bound to verified discovery".into(),
            ));
        }
        let existing_ids = manifest
            .candidate_targets
            .iter()
            .flat_map(|target| {
                target
                    .build_recipe_ref
                    .iter()
                    .chain(target.test_recipe_refs.iter())
                    .chain(target.start_recipe_ref.iter())
            })
            .collect::<std::collections::BTreeSet<_>>();
        if existing_ids.contains(&&recipe.recipe_id)
            || manifest
                .recipes
                .iter()
                .chain(manifest.user_overrides.iter())
                .any(|existing| existing.recipe_id == recipe.recipe_id)
        {
            return Err(StoreError::MalformedState(
                "trusted adapter recipe identifier is already in use".into(),
            ));
        }
        let existing_ids = super::target_recipe_ids(&before);
        let existing_recipes = manifest
            .recipes
            .iter()
            .chain(manifest.user_overrides.iter())
            .filter(|candidate| existing_ids.contains(&candidate.recipe_id.as_str()))
            .cloned()
            .collect::<Vec<_>>();
        if !existing_ids.is_empty() {
            let existing_origin =
                ProjectRuntimeStore::trusted_adapter_origin_sha256(&manifest, &before)
                    .map_err(StoreError::MalformedState)?;
            if !crate::persistence::runtime_target_approvals::has_trusted_adapter_recipe_origin(
                &manifest.workspace_id,
                &before.owning_task_id,
                &existing_origin,
            )
            .map_err(|error| StoreError::MalformedState(error.to_string()))?
            {
                return Err(StoreError::MalformedState(
                    "existing target recipes lack exact local adapter provenance".into(),
                ));
            }
        }
        let unique_existing_ids = existing_ids
            .iter()
            .copied()
            .collect::<std::collections::BTreeSet<_>>();
        if existing_recipes.len() != unique_existing_ids.len()
            || existing_recipes
                .iter()
                .any(|candidate| candidate.source != RecipeSource::TrustedAdapter)
        {
            return Err(StoreError::MalformedState(
                "trusted adapter target cannot carry unverified recipe sources".into(),
            ));
        }
        let next_revision = expected_manifest_revision.checked_add(1).ok_or_else(|| {
            StoreError::MalformedState("project runtime manifest revision is exhausted".into())
        })?;
        if manifest.revision != expected_manifest_revision {
            return Err(StoreError::StaleRecordRevision {
                path: super::super::MANIFEST_PATH.into(),
                expected: expected_manifest_revision,
                actual: manifest.revision,
            });
        }
        let target = &mut manifest.candidate_targets[target_index];
        match operation {
            RuntimeOperation::Build => target.build_recipe_ref = Some(recipe.recipe_id.clone()),
            RuntimeOperation::Test => target.test_recipe_refs.push(recipe.recipe_id.clone()),
            RuntimeOperation::Start => target.start_recipe_ref = Some(recipe.recipe_id.clone()),
        }
        target.capability_state = crate::project_runtime::TargetCapabilityState::ReadyButUntested;
        manifest.target_capabilities[target_index] = target.capability_state.clone();
        manifest.recipes.push(recipe);
        manifest.revision = next_revision;
        manifest.validate().map_err(StoreError::MalformedState)?;
        let target = &manifest.candidate_targets[target_index];
        let recipe_ids = super::target_recipe_ids(target);
        let recipes = manifest
            .recipes
            .iter()
            .chain(manifest.user_overrides.iter())
            .filter(|candidate| recipe_ids.contains(&candidate.recipe_id.as_str()))
            .cloned()
            .collect::<Vec<_>>();
        let readiness = target
            .readiness_probe_ref
            .as_ref()
            .and_then(|id| manifest.readiness_probes.get(id));
        let artifact_spec = target
            .artifact_spec_ref
            .as_ref()
            .and_then(|id| manifest.artifact_specs.get(id));
        let readiness_recipe = match readiness {
            Some(crate::project_runtime::ReadinessProbeV1::CustomApprovedProbe { recipe_id }) => {
                Some(
                    manifest
                        .recipes
                        .iter()
                        .chain(manifest.user_overrides.iter())
                        .find(|candidate| candidate.recipe_id == *recipe_id)
                        .ok_or_else(|| {
                            StoreError::MalformedState("trusted readiness recipe is missing".into())
                        })?,
                )
            }
            _ => None,
        };
        if readiness_recipe.is_some_and(|recipe| {
            recipe.source != crate::project_runtime::RecipeSource::TrustedAdapter
        }) {
            return Err(StoreError::MalformedState(
                "trusted custom readiness requires trusted adapter provenance".into(),
            ));
        }
        let origin_sha = ProjectRuntimeStore::trusted_adapter_recipe_sha256(
            &manifest.workspace_id,
            &manifest.source_sha,
            target,
            &recipes,
            readiness,
            readiness_recipe,
            artifact_spec,
        )
        .map_err(StoreError::MalformedState)?;
        crate::persistence::runtime_target_approvals::save_trusted_adapter_recipe_origin(
            &manifest.workspace_id,
            &target.owning_task_id,
            &origin_sha,
        )
        .map_err(|error| StoreError::MalformedState(error.to_string()))?;
        self.save_manifest(&manifest, expected_manifest_revision)?;
        Ok(manifest)
    }
}

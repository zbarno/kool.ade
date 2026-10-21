use super::super::ProjectRuntimeStore;
use crate::project_runtime::{
    AppTargetV1, ExecutionRecipeV1, ProjectCapabilityManifestV1, ReadinessProbeV1, RuntimeOperation,
};

use crate::{
    artifacts::planning_store::StoreError, persistence::runtime_target_approvals::Binding,
    project_runtime::RecipeSource,
};

use super::approval;
mod binding;

impl<'a> ProjectRuntimeStore<'a> {
    pub(crate) fn discovered_target_origin_sha256(
        workspace_id: &str,
        source_sha: &str,
        target: &AppTargetV1,
    ) -> Result<String, String> {
        approval::discovered_target_origin_sha256(workspace_id, source_sha, target)
    }

    pub(crate) fn trusted_adapter_recipe_sha256(
        workspace_id: &str,
        source_sha: &str,
        target: &AppTargetV1,
        recipes: &[ExecutionRecipeV1],
        readiness: Option<&super::super::super::ReadinessProbeV1>,
        readiness_recipe: Option<&ExecutionRecipeV1>,
        artifact_spec: Option<&Vec<String>>,
    ) -> Result<String, String> {
        approval::trusted_adapter_recipe_sha256(
            workspace_id,
            source_sha,
            target,
            recipes,
            readiness,
            readiness_recipe,
            artifact_spec,
        )
    }

    pub(crate) fn trusted_adapter_origin_sha256(
        manifest: &ProjectCapabilityManifestV1,
        target: &AppTargetV1,
    ) -> Result<String, String> {
        let recipe_ids = super::target_recipe_ids(target);
        let recipes = manifest
            .recipes
            .iter()
            .chain(manifest.user_overrides.iter())
            .filter(|recipe| recipe_ids.contains(&recipe.recipe_id.as_str()))
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
        let readiness_recipe = trusted_readiness_recipe(manifest, readiness)?;
        approval::trusted_adapter_recipe_sha256(
            &manifest.workspace_id,
            &manifest.source_sha,
            target,
            &recipes,
            readiness,
            readiness_recipe,
            artifact_spec,
        )
    }

    pub(crate) fn custom_target_origin_sha256(
        manifest: &ProjectCapabilityManifestV1,
        target: &AppTargetV1,
    ) -> Result<String, String> {
        let recipe_ids = super::target_recipe_ids(target);
        let recipes = manifest
            .recipes
            .iter()
            .chain(manifest.user_overrides.iter())
            .filter(|recipe| recipe_ids.contains(&recipe.recipe_id.as_str()))
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
        approval::custom_target_origin_sha256(target, &recipes, readiness, artifact_spec)
    }

    pub(crate) fn user_target_proposal_sha256(
        workspace_id: &str,
        repository_id: &str,
        source_sha: &str,
        target: &AppTargetV1,
        recipes: &[ExecutionRecipeV1],
        readiness: Option<&super::super::super::ReadinessProbeV1>,
        artifact_spec: Option<&Vec<String>>,
    ) -> Result<String, String> {
        approval::proposal_sha256(
            workspace_id,
            repository_id,
            source_sha,
            target,
            recipes,
            readiness,
            artifact_spec,
        )
    }

    pub(crate) fn user_recipe_approval_binding(
        manifest: &ProjectCapabilityManifestV1,
        target: &AppTargetV1,
        operation: RuntimeOperation,
        recipe: &ExecutionRecipeV1,
    ) -> Result<Binding, String> {
        let approval_id = recipe
            .evidence_refs
            .iter()
            .find_map(|reference| reference.strip_prefix("user-recipe-approval:"))
            .ok_or_else(|| "user-approved recipe has no local approval reference".to_owned())?;
        let readiness = if operation == RuntimeOperation::Start {
            target
                .readiness_probe_ref
                .as_ref()
                .and_then(|id| manifest.readiness_probes.get(id))
        } else {
            None
        };
        let readiness_recipe = match readiness {
            Some(ReadinessProbeV1::CustomApprovedProbe { recipe_id }) => Some(
                manifest
                    .recipes
                    .iter()
                    .chain(manifest.user_overrides.iter())
                    .find(|probe| probe.recipe_id == *recipe_id)
                    .ok_or_else(|| "approved readiness recipe is missing".to_owned())?,
            ),
            _ => None,
        };
        if readiness_recipe.is_some_and(|probe| probe.source != RecipeSource::UserApproved) {
            return Err("custom readiness probes require a locally user-approved recipe".into());
        }
        let artifact_spec = if matches!(operation, RuntimeOperation::Build | RuntimeOperation::Test)
        {
            target
                .artifact_spec_ref
                .as_ref()
                .and_then(|id| manifest.artifact_specs.get(id))
        } else {
            None
        };
        let proposal_sha256 = binding::user_recipe_approval_sha256(
            &manifest.workspace_id,
            &manifest.repository_id,
            &manifest.source_sha,
            target,
            operation,
            recipe,
            readiness,
            readiness_recipe,
            artifact_spec,
        )?;
        Ok(Binding {
            workspace_id: manifest.workspace_id.clone(),
            task_id: target.owning_task_id.clone(),
            repository_id: manifest.repository_id.clone(),
            source_sha: manifest.source_sha.clone(),
            target_id: target.target_id.clone(),
            proposal_sha256,
            receipt_id: approval_id.to_owned(),
        })
    }

    pub(crate) fn verify_user_recipe_approval(
        &self,
        manifest: &ProjectCapabilityManifestV1,
        target: &AppTargetV1,
        operation: RuntimeOperation,
        recipe: &ExecutionRecipeV1,
    ) -> Result<(), StoreError> {
        if recipe.source != RecipeSource::UserApproved {
            return Err(StoreError::MalformedState(
                "recipe does not carry user approval".into(),
            ));
        }
        let binding = Self::user_recipe_approval_binding(manifest, target, operation, recipe)
            .map_err(StoreError::MalformedState)?;
        if !crate::persistence::runtime_target_approvals::contains(&binding).map_err(|error| {
            StoreError::MalformedState(format!(
                "operator-local recipe approval could not be verified: {error}"
            ))
        })? {
            return Err(StoreError::MalformedState(
                "operator-local approval for this recipe is missing or revoked".into(),
            ));
        }
        Ok(())
    }

    pub(crate) fn verify_execution_provenance(
        &self,
        manifest: &ProjectCapabilityManifestV1,
        target: &AppTargetV1,
        operation: RuntimeOperation,
    ) -> Result<(), StoreError> {
        let custom_origin = Self::custom_target_origin_sha256(manifest, target)
            .map_err(StoreError::MalformedState)?;
        let is_custom_target = target.target_id.starts_with("user-target-")
            || crate::persistence::runtime_target_approvals::has_custom_target_origin(
                &manifest.workspace_id,
                &target.owning_task_id,
                &custom_origin,
            )
            .map_err(|error| StoreError::MalformedState(error.to_string()))?;
        if is_custom_target {
            self.verify_user_target_approval(manifest, target)?;
        }
        let recipe_ids = match operation {
            RuntimeOperation::Build => target.build_recipe_ref.iter().collect::<Vec<_>>(),
            RuntimeOperation::Test => target.test_recipe_refs.iter().collect::<Vec<_>>(),
            RuntimeOperation::Start => target.start_recipe_ref.iter().collect::<Vec<_>>(),
        };
        if recipe_ids.is_empty() {
            return Err(StoreError::MalformedState(
                "runtime target has no recipe for this operation".into(),
            ));
        }
        let mut has_trusted_adapter_recipe = false;
        for recipe_id in recipe_ids {
            let recipe = manifest
                .recipes
                .iter()
                .chain(manifest.user_overrides.iter())
                .find(|recipe| recipe.recipe_id == recipe_id.as_str())
                .ok_or_else(|| {
                    StoreError::MalformedState("runtime target recipe is missing".into())
                })?;
            match &recipe.source {
                RecipeSource::UserApproved => {
                    self.verify_user_recipe_approval(manifest, target, operation, recipe)?;
                }
                RecipeSource::TrustedAdapter => has_trusted_adapter_recipe = true,
                RecipeSource::ValidatedProjectConfig => {
                    return Err(StoreError::MalformedState(
                        "project-config recipe provenance is not independently validated".into(),
                    ));
                }
            }
        }
        if has_trusted_adapter_recipe {
            let origin = Self::trusted_adapter_origin_sha256(manifest, target)
                .map_err(StoreError::MalformedState)?;
            if !crate::persistence::runtime_target_approvals::has_trusted_adapter_recipe_origin(
                &manifest.workspace_id,
                &target.owning_task_id,
                &origin,
            )
            .map_err(|error| StoreError::MalformedState(error.to_string()))?
            {
                return Err(StoreError::MalformedState(
                    "trusted adapter recipe does not match verified local adapter provenance"
                        .into(),
                ));
            }
        }
        Ok(())
    }
}

fn trusted_readiness_recipe<'a>(
    manifest: &'a ProjectCapabilityManifestV1,
    readiness: Option<&ReadinessProbeV1>,
) -> Result<Option<&'a ExecutionRecipeV1>, String> {
    let Some(ReadinessProbeV1::CustomApprovedProbe { recipe_id }) = readiness else {
        return Ok(None);
    };
    let recipe = manifest
        .recipes
        .iter()
        .chain(manifest.user_overrides.iter())
        .find(|candidate| candidate.recipe_id == *recipe_id)
        .ok_or_else(|| "trusted readiness recipe is missing".to_owned())?;
    if recipe.source != RecipeSource::TrustedAdapter {
        return Err("trusted custom readiness requires trusted adapter provenance".into());
    }
    Ok(Some(recipe))
}

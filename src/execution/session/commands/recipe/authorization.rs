use super::super::super::{ExecutionSession, policy_error};
use crate::{
    execution::errors::ExecutionError,
    project_runtime::{
        AppTargetV1, ExecutionRecipeV1, ProjectCapabilityManifestV1, ProjectRuntimeStore,
        RecipeSource, RuntimeOperation,
    },
};

impl ExecutionSession {
    pub(crate) fn verify_local_recipe_approval(
        &self,
        runtime: &ProjectRuntimeStore<'_>,
        target_id: &str,
        recipe: &ExecutionRecipeV1,
        expected_root: Option<&str>,
    ) -> Result<String, ExecutionError> {
        let manifest = runtime
            .load_manifest()
            .map_err(|_| policy_error("Project runtime approval state is unavailable"))?
            .ok_or_else(|| policy_error("Project runtime approval state is missing"))?;
        let workspace = self.workspace();
        if manifest.workspace_id != workspace.project_id
            || manifest.source_sha != workspace.source_sha
        {
            return Err(policy_error(
                "Project runtime approval does not match the pinned task workspace",
            ));
        }
        let target = manifest
            .candidate_targets
            .iter()
            .find(|target| target.target_id == target_id)
            .ok_or_else(|| policy_error("Approved runtime target is missing"))?;
        if target.owning_task_id != workspace.task_id
            || target.repository_id != manifest.repository_id
            || expected_root.is_some_and(|root| target.project_relative_root != root)
            || !manifest.is_active_target(&workspace.task_id, target_id)
            || !target
                .build_recipe_ref
                .iter()
                .chain(target.test_recipe_refs.iter())
                .chain(target.start_recipe_ref.iter())
                .any(|recipe_id| recipe_id == &recipe.recipe_id)
            || !approved_recipe_matches(runtime, &manifest, target, recipe)
        {
            return Err(policy_error(
                "Recipe has no matching local approval for this selected task target",
            ));
        }
        match &recipe.source {
            RecipeSource::UserApproved => recipe
                .evidence_refs
                .iter()
                .find(|reference| reference.starts_with("user-approval:"))
                .cloned()
                .ok_or_else(|| policy_error("Recipe approval evidence is missing")),
            RecipeSource::TrustedAdapter => {
                let origin = ProjectRuntimeStore::trusted_adapter_origin_sha256(&manifest, target)
                    .map_err(|_| policy_error("Trusted adapter provenance is unavailable"))?;
                Ok(format!("trusted-adapter-provenance:{origin}"))
            }
            RecipeSource::ValidatedProjectConfig => Err(policy_error(
                "Project-config recipes require independent validation before execution",
            )),
        }
    }
}

fn approved_recipe_matches(
    runtime: &ProjectRuntimeStore<'_>,
    manifest: &ProjectCapabilityManifestV1,
    target: &AppTargetV1,
    recipe: &ExecutionRecipeV1,
) -> bool {
    let stored_recipe = manifest
        .recipes
        .iter()
        .chain(manifest.user_overrides.iter())
        .find(|candidate| candidate.recipe_id == recipe.recipe_id);
    let mut approved = recipe.clone();
    let root = &target.project_relative_root;
    let normalize = |path: &str| {
        if root == "." {
            Ok(path.to_owned())
        } else {
            super::target::strip_target_root(root, path)
        }
    };
    let Ok(working_directory_relative) = normalize(&recipe.working_directory_relative) else {
        return false;
    };
    approved.working_directory_relative = working_directory_relative;
    approved.allowed_output_paths = match recipe
        .allowed_output_paths
        .iter()
        .map(|path| normalize(path))
        .collect::<Result<Vec<_>, _>>()
    {
        Ok(paths) => paths,
        Err(_) => return false,
    };
    approved.expected_artifacts = match recipe
        .expected_artifacts
        .iter()
        .map(|path| normalize(path))
        .collect::<Result<Vec<_>, _>>()
    {
        Ok(paths) => paths,
        Err(_) => return false,
    };
    if stored_recipe != Some(&approved) {
        return false;
    }
    match &recipe.source {
        RecipeSource::TrustedAdapter => recipe_operation(target, recipe).is_some_and(|operation| {
            runtime
                .verify_execution_provenance(manifest, target, operation)
                .is_ok()
        }),
        RecipeSource::UserApproved => {
            runtime.recipe_is_locally_approved(manifest, target, &approved)
        }
        RecipeSource::ValidatedProjectConfig => false,
    }
}

fn recipe_operation(target: &AppTargetV1, recipe: &ExecutionRecipeV1) -> Option<RuntimeOperation> {
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

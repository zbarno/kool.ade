use super::*;

impl ExecutionSession {
    /// Executes an approved recipe without a shell or script interpolation.
    pub fn run_recipe(
        &self,
        runtime: &crate::project_runtime::ProjectRuntimeStore<'_>,
        target_id: &str,
        recipe: &crate::project_runtime::ExecutionRecipeV1,
    ) -> Result<ExecutionOutput, ExecutionError> {
        let manifest = runtime
            .load_manifest()
            .map_err(|_| policy_error("Project runtime approval state is unavailable"))?
            .ok_or_else(|| policy_error("Project runtime approval state is missing"))?;
        let target = manifest
            .candidate_targets
            .iter()
            .find(|target| target.target_id == target_id)
            .ok_or_else(|| policy_error("Approved runtime target is missing"))?;
        let mut workspace_recipe = recipe.clone();
        workspace_recipe.working_directory_relative = join_target_root(
            &target.project_relative_root,
            &recipe.working_directory_relative,
        )?;
        workspace_recipe.allowed_output_paths = recipe
            .allowed_output_paths
            .iter()
            .map(|path| join_target_root(&target.project_relative_root, path))
            .collect::<Result<_, _>>()?;
        workspace_recipe.expected_artifacts = recipe
            .expected_artifacts
            .iter()
            .map(|path| join_target_root(&target.project_relative_root, path))
            .collect::<Result<_, _>>()?;
        self.run_scoped_recipe(
            runtime,
            target_id,
            &workspace_recipe,
            &target.project_relative_root,
        )
    }

    /// Runs one explicitly approved recipe with a task clone mounted as only
    /// the selected target subtree. The request carries an exact argv grant.
    pub fn run_scoped_recipe(
        &self,
        runtime: &crate::project_runtime::ProjectRuntimeStore<'_>,
        target_id: &str,
        recipe: &crate::project_runtime::ExecutionRecipeV1,
        target_root: &str,
    ) -> Result<ExecutionOutput, ExecutionError> {
        self.run_scoped_recipe_with_artifacts(runtime, target_id, recipe, target_root, &[])
    }

    /// Runs an approved scoped recipe and returns hashes for the exact
    /// application-selected artifact paths in its container workspace.
    pub fn run_scoped_recipe_with_artifacts(
        &self,
        runtime: &crate::project_runtime::ProjectRuntimeStore<'_>,
        target_id: &str,
        recipe: &crate::project_runtime::ExecutionRecipeV1,
        target_root: &str,
        artifact_paths: &[String],
    ) -> Result<ExecutionOutput, ExecutionError> {
        if let Some(destination) = self.retry_destination()? {
            return destination.run_scoped_recipe_with_artifacts(
                runtime,
                target_id,
                recipe,
                target_root,
                artifact_paths,
            );
        }
        use crate::execution::policy::ExecutionPolicyV1;
        use std::collections::BTreeSet;

        self.renew_remote_capacity_lease()?;
        let authorization_ref =
            self.verify_local_recipe_approval(runtime, target_id, recipe, Some(target_root))?;
        recipe
            .validate()
            .map_err(|_| policy_error("Project execution recipe failed validation"))?;
        if self.mode != crate::execution::contract::ExecutionModeV1::Verification {
            return Err(policy_error(
                "Scoped recipe execution requires verification mode",
            ));
        }
        let working_directory = strip_target_root(target_root, &recipe.working_directory_relative)?;
        validate_artifact_outputs(recipe, target_root, artifact_paths)?;
        let environment = recipe
            .allowed_env_keys
            .iter()
            .cloned()
            .collect::<BTreeSet<_>>();
        let policy = match &recipe.source {
            crate::project_runtime::RecipeSource::UserApproved => {
                ExecutionPolicyV1::for_approved_recipe(
                    self.resources.clone(),
                    target_root.to_owned(),
                    authorization_ref,
                    recipe.executable_id.clone(),
                    recipe.argv.clone(),
                    working_directory.clone(),
                    environment,
                )
            }
            crate::project_runtime::RecipeSource::TrustedAdapter => {
                ExecutionPolicyV1::for_trusted_adapter_recipe(
                    self.resources.clone(),
                    target_root.to_owned(),
                    authorization_ref,
                    recipe.executable_id.clone(),
                    recipe.argv.clone(),
                    working_directory.clone(),
                    environment,
                )
            }
            crate::project_runtime::RecipeSource::ValidatedProjectConfig => {
                return Err(policy_error(
                    "Project-config recipes require independent validation before execution",
                ));
            }
        }
        .map_err(|_| policy_error("Scoped recipe authorization is invalid"))?;
        let policy = policy
            .with_source_output_paths(recipe_source_output_paths(recipe))
            .map_err(|_| policy_error("Scoped recipe output authorization is invalid"))?;
        if self
            .busy
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
        {
            return Err(policy_error(
                "An execution is already active in this session",
            ));
        }
        self.cancel_requested.store(false, Ordering::Release);
        let sequence = self.sequence.fetch_add(1, Ordering::Relaxed);
        let cancellation_generation = self.cancellation_generation.fetch_add(1, Ordering::AcqRel);
        let mut argv = vec![recipe.executable_id.clone()];
        argv.extend(recipe.argv.iter().cloned());
        let request = self.request(RequestParameters {
            argv,
            stdin: None,
            working_directory_relative: working_directory,
            environment_allowlist: recipe.allowed_env_keys.iter().cloned().collect(),
            timeout: Duration::from_secs(recipe.timeout_seconds),
            request_sequence: sequence,
            cancellation_generation,
            policy_override: Some(policy),
            artifact_paths: artifact_paths.to_vec(),
        });
        let request = match request {
            Ok(request) => request,
            Err(error) => {
                self.busy.store(false, Ordering::Release);
                return Err(error);
            }
        };
        self.verify_authorization()?;
        let result = self.runtime.block_on(self.run_request(request));
        self.clear_active();
        let confirmed_loss = result
            .as_ref()
            .is_ok_and(|output| super::super::is_confirmed_remote_worker_loss(&output.result));
        if result.is_ok() && !confirmed_loss {
            self.verify_authorization()?;
        }
        self.retry_after_confirmed_loss_with(result, |destination| {
            destination.run_scoped_recipe_with_artifacts(
                runtime,
                target_id,
                recipe,
                target_root,
                artifact_paths,
            )
        })
    }

    pub(crate) fn verify_local_recipe_approval(
        &self,
        runtime: &crate::project_runtime::ProjectRuntimeStore<'_>,
        target_id: &str,
        recipe: &crate::project_runtime::ExecutionRecipeV1,
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
            crate::project_runtime::RecipeSource::UserApproved => recipe
                .evidence_refs
                .iter()
                .find(|reference| reference.starts_with("user-approval:"))
                .cloned()
                .ok_or_else(|| policy_error("Recipe approval evidence is missing")),
            crate::project_runtime::RecipeSource::TrustedAdapter => {
                let origin =
                    crate::project_runtime::ProjectRuntimeStore::trusted_adapter_origin_sha256(
                        &manifest, target,
                    )
                    .map_err(|_| policy_error("Trusted adapter provenance is unavailable"))?;
                Ok(format!("trusted-adapter-provenance:{origin}"))
            }
            crate::project_runtime::RecipeSource::ValidatedProjectConfig => Err(policy_error(
                "Project-config recipes require independent validation before execution",
            )),
        }
    }
}

pub(super) fn recipe_source_output_paths(
    recipe: &crate::project_runtime::ExecutionRecipeV1,
) -> Vec<String> {
    recipe
        .allowed_output_paths
        .iter()
        .chain(recipe.expected_artifacts.iter())
        .cloned()
        .collect()
}

fn approved_recipe_matches(
    runtime: &crate::project_runtime::ProjectRuntimeStore<'_>,
    manifest: &crate::project_runtime::ProjectCapabilityManifestV1,
    target: &crate::project_runtime::AppTargetV1,
    recipe: &crate::project_runtime::ExecutionRecipeV1,
) -> bool {
    let mut approved = recipe.clone();
    let root = &target.project_relative_root;
    let normalize = |path: &str| {
        if root == "." {
            Ok(path.to_owned())
        } else {
            strip_target_root(root, path)
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
    runtime.recipe_is_locally_approved(manifest, target, &approved)
}

fn join_target_root(target_root: &str, path: &str) -> Result<String, ExecutionError> {
    if target_root == "." {
        Ok(path.to_owned())
    } else if path == "." {
        Ok(target_root.to_owned())
    } else {
        Ok(format!("{target_root}/{path}"))
    }
}

pub(super) fn strip_target_root(target_root: &str, path: &str) -> Result<String, ExecutionError> {
    if target_root == "." {
        return Ok(path.to_owned());
    }
    let relative = path
        .strip_prefix(target_root)
        .filter(|suffix| suffix.is_empty() || suffix.starts_with('/'))
        .ok_or_else(|| {
            policy_error("Recipe working directory is outside its approved target root")
        })?;
    let relative = relative.strip_prefix('/').unwrap_or(relative);
    if relative.is_empty() {
        Ok(".".into())
    } else {
        Ok(relative.to_owned())
    }
}

pub(super) fn validate_artifact_outputs(
    recipe: &crate::project_runtime::ExecutionRecipeV1,
    target_root: &str,
    artifact_paths: &[String],
) -> Result<(), ExecutionError> {
    for artifact in artifact_paths {
        let allowed = recipe
            .allowed_output_paths
            .iter()
            .chain(recipe.expected_artifacts.iter())
            .filter_map(|path| strip_target_root(target_root, path).ok())
            .any(|root| {
                root == "."
                    || artifact == &root
                    || artifact
                        .strip_prefix(&root)
                        .is_some_and(|suffix| suffix.starts_with('/'))
            });
        if !allowed {
            return Err(policy_error(
                "Requested artifact is outside the approved recipe outputs",
            ));
        }
    }
    Ok(())
}

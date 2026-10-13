use std::{collections::BTreeSet, sync::atomic::Ordering, time::Duration};

use crate::execution::errors::ExecutionError;

mod qa;

use super::{ExecutionOutput, ExecutionSession, RequestParameters, policy_error};

impl ExecutionSession {
    /// Executes a bounded shell script on stdin inside the selected execution
    /// backend. The shell and script are never interpolated into a host command.
    pub fn run_bounded_script(
        &self,
        script: &str,
        timeout: Duration,
    ) -> Result<ExecutionOutput, ExecutionError> {
        self.verify_authorization()?;
        self.renew_remote_capacity_lease()?;
        if script.len() > crate::execution::contract::MAX_SCRIPT_BYTES
            || script.contains('\0')
            || timeout.is_zero()
            || timeout > Duration::from_secs(1200)
        {
            return Err(policy_error(
                "Shell script or execution timeout is outside limits",
            ));
        }
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
        let request = match self.request(RequestParameters {
            argv: vec!["sh".into(), "-s".into()],
            stdin: Some(script.to_owned()),
            working_directory_relative: ".".into(),
            environment_allowlist: BTreeSet::new(),
            timeout,
            request_sequence: sequence,
            cancellation_generation,
            policy_override: None,
        }) {
            Ok(request) => request,
            Err(error) => {
                self.busy.store(false, Ordering::Release);
                return Err(error);
            }
        };
        let result = self.runtime.block_on(self.run_request(request));
        self.clear_active();
        if result.is_ok() {
            self.verify_authorization()?;
        }
        result
    }

    /// Executes an approved recipe without a shell or script interpolation.
    pub fn run_recipe(
        &self,
        recipe: &crate::project_runtime::ExecutionRecipeV1,
    ) -> Result<ExecutionOutput, ExecutionError> {
        self.renew_remote_capacity_lease()?;
        recipe
            .validate()
            .map_err(|_| policy_error("Project execution recipe failed validation"))?;
        if !matches!(
            recipe.source,
            crate::project_runtime::RecipeSource::UserApproved
                | crate::project_runtime::RecipeSource::TrustedAdapter
        ) {
            return Err(policy_error(
                "Project recipe requires explicit approval before execution",
            ));
        }
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
        let mut argv = Vec::with_capacity(recipe.argv.len() + 1);
        argv.push(recipe.executable_id.clone());
        argv.extend(recipe.argv.iter().cloned());
        let request = self.request(RequestParameters {
            argv,
            stdin: None,
            working_directory_relative: recipe.working_directory_relative.clone(),
            environment_allowlist: recipe.allowed_env_keys.iter().cloned().collect(),
            timeout: Duration::from_secs(recipe.timeout_seconds),
            request_sequence: sequence,
            cancellation_generation,
            policy_override: None,
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
        if result.is_ok() {
            self.verify_authorization()?;
        }
        result
    }

    /// Runs one explicitly approved recipe with a task clone mounted as only
    /// the selected target subtree. The request carries an exact argv grant.
    pub fn run_scoped_recipe(
        &self,
        recipe: &crate::project_runtime::ExecutionRecipeV1,
        target_root: &str,
    ) -> Result<ExecutionOutput, ExecutionError> {
        use crate::execution::policy::ExecutionPolicyV1;
        use std::collections::BTreeSet;

        self.renew_remote_capacity_lease()?;
        recipe
            .validate()
            .map_err(|_| policy_error("Project execution recipe failed validation"))?;
        if self.mode != crate::execution::contract::ExecutionModeV1::Verification
            || recipe.source != crate::project_runtime::RecipeSource::UserApproved
        {
            return Err(policy_error(
                "Scoped recipe execution requires an explicitly user-approved verification recipe",
            ));
        }
        let approval = recipe
            .evidence_refs
            .iter()
            .find(|reference| reference.starts_with("user-approval:"))
            .cloned()
            .ok_or_else(|| policy_error("Recipe approval evidence is missing"))?;
        let working_directory = strip_target_root(target_root, &recipe.working_directory_relative)?;
        let policy = ExecutionPolicyV1::for_approved_recipe(
            self.resources.clone(),
            target_root.to_owned(),
            approval,
            recipe.executable_id.clone(),
            recipe.argv.clone(),
            working_directory.clone(),
            recipe
                .allowed_env_keys
                .iter()
                .cloned()
                .collect::<BTreeSet<_>>(),
        )
        .map_err(|_| policy_error("Scoped recipe authorization is invalid"))?;
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
        if result.is_ok() {
            self.verify_authorization()?;
        }
        result
    }
}

fn strip_target_root(target_root: &str, path: &str) -> Result<String, ExecutionError> {
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

#[cfg(test)]
#[path = "commands/tests.rs"]
mod tests;

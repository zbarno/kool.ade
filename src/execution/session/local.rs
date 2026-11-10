use std::{path::Path, sync::Arc, time::Duration};

use crate::execution::{
    backend::ExecutionBackend,
    contract::{ExecutionModeV1, ExecutionTarget, ResourceBudgetV1, WorkspaceRef},
    docker::{
        backend::{
            DockerExecutionBackend, OwnedTaskWorkspaceResolver, ReadOnlyProjectWorkspaceResolver,
        },
        engine::DockerEndpoint,
        images::{ImageLock, ProjectRuntimeImagePlan, RuntimePlanBinding, RuntimeProfile},
    },
    errors::{ExecutionError, ExecutionErrorKind},
};

use super::{
    AuthorizationGenerationVerifier, ExecutionSession, RuntimeProvenance, current_platform,
};

impl ExecutionSession {
    /// Creates a local Docker session for an application-selected task clone.
    pub fn connect_local(
        workspace: WorkspaceRef,
        task_clone: &Path,
        mode: ExecutionModeV1,
        profile: RuntimeProfile,
        resources: ResourceBudgetV1,
    ) -> Result<Self, ExecutionError> {
        Self::connect_local_with_authorization(
            workspace, task_clone, mode, profile, resources, None,
        )
    }

    pub fn connect_local_with_authorization(
        workspace: WorkspaceRef,
        task_clone: &Path,
        mode: ExecutionModeV1,
        profile: RuntimeProfile,
        resources: ResourceBudgetV1,
        authorization_verifier: Option<Arc<dyn AuthorizationGenerationVerifier>>,
    ) -> Result<Self, ExecutionError> {
        Self::connect_local_with_destination_sha_and_authorization(
            workspace,
            task_clone,
            mode,
            profile,
            resources,
            None,
            authorization_verifier,
        )
    }

    pub(crate) fn connect_local_with_destination_sha_and_authorization(
        workspace: WorkspaceRef,
        task_clone: &Path,
        mode: ExecutionModeV1,
        profile: RuntimeProfile,
        resources: ResourceBudgetV1,
        destination_sha: Option<String>,
        authorization_verifier: Option<Arc<dyn AuthorizationGenerationVerifier>>,
    ) -> Result<Self, ExecutionError> {
        connect_local_with_image_selection(
            workspace,
            task_clone,
            mode,
            &[profile],
            &[],
            resources,
            destination_sha,
            authorization_verifier,
            None,
        )
    }

    /// Creates a local task session using the first pinned runtime image that
    /// provides every logical tool required by an approved recipe.
    pub fn connect_local_with_tools(
        workspace: WorkspaceRef,
        task_clone: &Path,
        mode: ExecutionModeV1,
        required_tools: &[String],
        resources: ResourceBudgetV1,
    ) -> Result<Self, ExecutionError> {
        const PROFILES: &[RuntimeProfile] = &[
            RuntimeProfile::General,
            RuntimeProfile::Rust,
            RuntimeProfile::Node,
            RuntimeProfile::Python,
            RuntimeProfile::Go,
        ];
        connect_local_with_image_selection(
            workspace,
            task_clone,
            mode,
            PROFILES,
            required_tools,
            resources,
            None,
            None,
            None,
        )
    }

    /// Creates a read-only Docker session for the repository selected by the
    /// application. Its source revision is checked again before each mount.
    pub fn connect_read_only_local(
        workspace: WorkspaceRef,
        repository_root: &Path,
        mode: ExecutionModeV1,
        profile: RuntimeProfile,
        resources: ResourceBudgetV1,
    ) -> Result<Self, ExecutionError> {
        if !matches!(
            mode,
            ExecutionModeV1::PlanningReadOnly | ExecutionModeV1::Verification
        ) {
            return Err(ExecutionError::new(
                ExecutionErrorKind::PolicyDenied,
                "Read-only workspace resolver cannot be used for this execution mode",
                false,
            ));
        }
        let endpoint =
            DockerEndpoint::from_process_environment(current_platform()).map_err(prerequisite)?;
        let state_root = crate::persistence::state_root();
        let staging = state_root.join("execution-staging");
        std::fs::create_dir_all(&staging).map_err(|_| prerequisite_message())?;
        let resolver = Arc::new(ReadOnlyProjectWorkspaceResolver::new(
            workspace.clone(),
            repository_root,
            &staging,
        )?);
        connect_with_resolver(
            endpoint, resolver, workspace, mode, profile, resources, None,
        )
    }
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn connect_local_with_image_selection(
    workspace: WorkspaceRef,
    task_clone: &Path,
    mode: ExecutionModeV1,
    profiles: &[RuntimeProfile],
    required_tools: &[String],
    resources: ResourceBudgetV1,
    destination_sha: Option<String>,
    authorization_verifier: Option<Arc<dyn AuthorizationGenerationVerifier>>,
    plan_binding: Option<&RuntimePlanBinding>,
) -> Result<ExecutionSession, ExecutionError> {
    let endpoint =
        DockerEndpoint::from_process_environment(current_platform()).map_err(prerequisite)?;
    let resolver = Arc::new(OwnedTaskWorkspaceResolver::for_current_user(
        workspace.clone(),
        task_clone,
    )?);
    let owner_instance_id = crate::persistence::execution_records::owner_instance_id()
        .map_err(|_| prerequisite_message())?;
    let backend = Arc::new(DockerExecutionBackend::connect_with_authorization(
        endpoint,
        resolver,
        owner_instance_id,
        authorization_verifier.clone(),
    )?);
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|_| prerequisite_message())?;
    // This is a read-only preflight: no execution session or command exists
    // yet. A rootless Engine can accept its first ping before its API stack is
    // ready, so retry only the typed health-disconnect failure here. Never
    // replay a command after `start` has been attempted.
    let probe = retry_local_engine_health_probe(|| {
        runtime.block_on(backend.probe(&ExecutionTarget::local()))
    })?;
    if !probe.available || !probe.compatible {
        return Err(ExecutionError::new(
            ExecutionErrorKind::EnvironmentUnavailable,
            "Docker Engine does not meet the local execution requirements",
            true,
        ));
    }
    let architecture = probe.architecture.ok_or_else(prerequisite_message)?;
    let lock = ImageLock::bundled().map_err(|error| {
        ExecutionError::new(
            ExecutionErrorKind::ImageUnavailable,
            error.message,
            error.retryable,
        )
    })?;
    let mut recipe_digest: Option<String> = None;
    let image = match lock.select_with_tools(profiles, &architecture, required_tools) {
        Ok(image) => image,
        Err(original) => match lock.plan_project_runtime(&architecture, required_tools) {
            Ok(ProjectRuntimeImagePlan::Composition(candidate)) => {
                // All inputs are from the reviewed image lock, but the
                // builder must independently verify their local identity,
                // execute the fixed offline build and smoke-test its tools.
                // No host fallback or synthetic image digest is allowed.
                let recipe_digest_assignment = Some(candidate.recipe_sha256.clone());
                let composed = runtime.block_on(backend.compose_project_runtime(&candidate));
                let image = composed
                    .map_err(|error| {
                        // The recipe queue can recover missing system tools
                        // through a separately authorized broker. A failed
                        // composite build or verification must NOT take that
                        // fallback, which could mask a trusted-image failure.
                        let tool_hint = candidate
                            .required_tools
                            .iter()
                            .take(4)
                            .cloned()
                            .collect::<Vec<_>>()
                            .join(", ");
                        let estimate = lock
                            .composition_size_estimate(&candidate)
                            .map(|estimate| {
                                format!(
                                    ", estimated <= {} MiB total image storage",
                                    estimate.estimated_storage_bytes.div_ceil(1 << 20)
                                )
                            })
                            .unwrap_or_default();
                        ExecutionError::new(
                            ExecutionErrorKind::PolicyDenied,
                            format!(
                                "Trusted composite runtime preparation failed for {tool_hint}{estimate}: {}. Resolve the reported blocker (typically Docker storage or a missing trusted source image) and retry; if preparation keeps failing, clear the application's composite runtime cache and try again.",
                                error.message
                            ),
                            error.retryable,
                        )
                    })?;
                recipe_digest = recipe_digest_assignment;
                image
            }
            Ok(ProjectRuntimeImagePlan::Existing(_)) => {
                return Err(ExecutionError::new(
                    ExecutionErrorKind::ImageUnavailable,
                    original.message,
                    original.retryable,
                ));
            }
            Err(error) => {
                return Err(ExecutionError::new(
                    ExecutionErrorKind::ImageUnavailable,
                    error.message,
                    error.retryable,
                ));
            }
        },
    };
    let mut session = ExecutionSession::new_with_destination_authorization(
        backend,
        workspace,
        ExecutionTarget::local(),
        mode,
        image.digest,
        resources,
        destination_sha,
        authorization_verifier,
    )
    .map_err(|_| prerequisite_message())?;
    if plan_binding.is_some() || recipe_digest.is_some() {
        session = session.with_runtime_provenance(Some(RuntimeProvenance {
            recipe_digest,
            binding_digest: plan_binding.map(RuntimePlanBinding::binding_digest),
        }));
    }
    Ok(session)
}

fn connect_with_resolver(
    endpoint: DockerEndpoint,
    resolver: Arc<dyn crate::execution::docker::backend::TaskWorkspaceResolver>,
    workspace: WorkspaceRef,
    mode: ExecutionModeV1,
    profile: RuntimeProfile,
    resources: ResourceBudgetV1,
    authorization_verifier: Option<Arc<dyn AuthorizationGenerationVerifier>>,
) -> Result<ExecutionSession, ExecutionError> {
    let owner_instance_id = crate::persistence::execution_records::owner_instance_id()
        .map_err(|_| prerequisite_message())?;
    let backend = Arc::new(DockerExecutionBackend::connect_with_authorization(
        endpoint,
        resolver,
        owner_instance_id,
        authorization_verifier.clone(),
    )?);
    let probe = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|_| prerequisite_message())?
        .block_on(backend.probe(&ExecutionTarget::local()))?;
    if !probe.available || !probe.compatible {
        return Err(ExecutionError::new(
            ExecutionErrorKind::EnvironmentUnavailable,
            "Docker Engine does not meet the local execution requirements",
            true,
        ));
    }
    let architecture = probe.architecture.ok_or_else(prerequisite_message)?;
    let image = ImageLock::bundled()
        .map_err(|error| {
            ExecutionError::new(
                ExecutionErrorKind::ImageUnavailable,
                error.message,
                error.retryable,
            )
        })?
        .select(profile, &architecture)
        .map_err(|error| {
            ExecutionError::new(
                ExecutionErrorKind::ImageUnavailable,
                error.message,
                error.retryable,
            )
        })?;
    ExecutionSession::new_with_authorization(
        backend,
        workspace,
        ExecutionTarget::local(),
        mode,
        image.digest,
        resources,
        authorization_verifier,
    )
    .map_err(|_| prerequisite_message())
}

fn retry_local_engine_health_probe<T>(
    mut probe: impl FnMut() -> Result<T, ExecutionError>,
) -> Result<T, ExecutionError> {
    const MAX_ATTEMPTS: usize = 3;
    const RETRY_DELAY: Duration = Duration::from_millis(250);

    for attempt in 1..=MAX_ATTEMPTS {
        match probe() {
            Err(error)
                if error.is_retryable_local_docker_health_disconnect()
                    && attempt < MAX_ATTEMPTS =>
            {
                std::thread::sleep(RETRY_DELAY);
            }
            result => return result,
        }
    }
    unreachable!("local engine health probe loop always returns")
}

fn prerequisite(
    failure: crate::execution::docker::discovery::PrerequisiteFailure,
) -> ExecutionError {
    ExecutionError::new(
        ExecutionErrorKind::EnvironmentUnavailable,
        failure.message,
        failure.retryable,
    )
}

fn prerequisite_message() -> ExecutionError {
    ExecutionError::new(
        ExecutionErrorKind::EnvironmentUnavailable,
        "Local Docker execution prerequisites are unavailable",
        true,
    )
}

#[cfg(test)]
mod health_probe_retry_tests {
    use super::*;
    use crate::execution::errors::{ExecutionErrorKind, LOCAL_DOCKER_HEALTH_DISCONNECTED};
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[test]
    fn retries_transient_docker_health_failure_before_session_creation() {
        let calls = AtomicUsize::new(0);
        let result = retry_local_engine_health_probe(|| {
            if calls.fetch_add(1, Ordering::SeqCst) == 0 {
                Err(ExecutionError::new(
                    ExecutionErrorKind::EnvironmentUnavailable,
                    LOCAL_DOCKER_HEALTH_DISCONNECTED,
                    true,
                ))
            } else {
                Ok("ready")
            }
        });

        assert_eq!(result.unwrap(), "ready");
        assert_eq!(calls.load(Ordering::SeqCst), 2);
    }

    #[test]
    fn does_not_retry_non_health_failures_or_non_retryable_health_failures() {
        for error in [
            ExecutionError::new(
                ExecutionErrorKind::EnvironmentUnavailable,
                "Docker Engine is unavailable",
                true,
            ),
            ExecutionError::new(
                ExecutionErrorKind::EnvironmentUnavailable,
                LOCAL_DOCKER_HEALTH_DISCONNECTED,
                false,
            ),
        ] {
            let calls = AtomicUsize::new(0);
            let result = retry_local_engine_health_probe(|| {
                calls.fetch_add(1, Ordering::SeqCst);
                Err::<(), _>(error.clone())
            });

            let returned = result.unwrap_err();
            assert_eq!(returned.kind, error.kind);
            assert_eq!(returned.message, error.message);
            assert_eq!(returned.retryable, error.retryable);
            assert_eq!(calls.load(Ordering::SeqCst), 1);
        }
    }

    #[test]
    fn stops_after_three_consecutive_health_probe_failures() {
        let calls = AtomicUsize::new(0);
        let result = retry_local_engine_health_probe(|| {
            calls.fetch_add(1, Ordering::SeqCst);
            Err::<(), _>(ExecutionError::new(
                ExecutionErrorKind::EnvironmentUnavailable,
                LOCAL_DOCKER_HEALTH_DISCONNECTED,
                true,
            ))
        });

        let error = result.unwrap_err();
        assert!(error.is_retryable_local_docker_health_disconnect());
        assert_eq!(calls.load(Ordering::SeqCst), 3);
    }
}

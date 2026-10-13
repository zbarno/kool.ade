use std::{path::Path, sync::Arc};

use crate::execution::{
    backend::ExecutionBackend,
    contract::{ExecutionModeV1, ExecutionTarget, ResourceBudgetV1, WorkspaceRef},
    docker::{
        backend::{
            DockerExecutionBackend, OwnedTaskWorkspaceResolver, ReadOnlyProjectWorkspaceResolver,
        },
        engine::DockerEndpoint,
        images::{ImageLock, RuntimeProfile},
    },
    errors::{ExecutionError, ExecutionErrorKind},
};

use super::{AuthorizationGenerationVerifier, ExecutionSession, current_platform};

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
        Self::new_with_authorization(
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

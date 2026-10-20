use std::path::Path;
use std::sync::Arc;

use crate::execution::{
    backend::ExecutionBackend,
    contract::{ExecutionModeV1, ExecutionTarget, ResourceBudgetV1, TargetLocation, WorkspaceRef},
    docker::images::{ImageLock, RuntimeProfile},
    errors::{ExecutionError, ExecutionErrorKind},
    remote::{backend::RemoteExecutionBackend, ssh::SshWorkerConfig},
};

use super::{AuthorizationGenerationVerifier, ExecutionSession};

pub struct RemoteSessionOptions<'a> {
    pub target: ExecutionTarget,
    pub worker: &'a SshWorkerConfig,
    pub mode: ExecutionModeV1,
    pub profile: RuntimeProfile,
    pub resources: ResourceBudgetV1,
    pub destination_sha: Option<String>,
    pub authorization_verifier: Option<Arc<dyn AuthorizationGenerationVerifier>>,
    pub task_clone: Option<&'a Path>,
    pub runtime_config_source: Option<&'a Path>,
}

impl ExecutionSession {
    /// Opens a bounded session to the configured SSH worker after host-key and
    /// Docker policy/capability negotiation.
    pub fn connect_remote(
        workspace: WorkspaceRef,
        target: ExecutionTarget,
        worker: &SshWorkerConfig,
        mode: ExecutionModeV1,
        profile: RuntimeProfile,
        resources: ResourceBudgetV1,
    ) -> Result<Self, ExecutionError> {
        Self::connect_remote_with_verifier(
            workspace,
            RemoteSessionOptions {
                target,
                worker,
                mode,
                profile,
                resources,
                destination_sha: None,
                authorization_verifier: None,
                task_clone: None,
                runtime_config_source: None,
            },
        )
    }

    /// Opens a remote session fenced by the app-owned task authorization
    /// generation. The same verifier runs before execution and before accepting
    /// worker results.
    pub fn connect_remote_with_authorization(
        workspace: WorkspaceRef,
        target: ExecutionTarget,
        worker: &SshWorkerConfig,
        mode: ExecutionModeV1,
        profile: RuntimeProfile,
        resources: ResourceBudgetV1,
        authorization_verifier: Arc<dyn AuthorizationGenerationVerifier>,
    ) -> Result<Self, ExecutionError> {
        Self::connect_remote_with_verifier(
            workspace,
            RemoteSessionOptions {
                target,
                worker,
                mode,
                profile,
                resources,
                destination_sha: None,
                authorization_verifier: Some(authorization_verifier),
                task_clone: None,
                runtime_config_source: None,
            },
        )
    }

    /// Opens a remote session and stages the app-owned task clone at the
    /// supplied path. The path is validated against the current user's project
    /// storage before any bytes are sent to the worker.
    pub fn connect_remote_with_task_clone(
        workspace: WorkspaceRef,
        options: RemoteSessionOptions<'_>,
    ) -> Result<Self, ExecutionError> {
        Self::connect_remote_with_verifier(workspace, options)
    }

    fn connect_remote_with_verifier(
        workspace: WorkspaceRef,
        options: RemoteSessionOptions<'_>,
    ) -> Result<Self, ExecutionError> {
        let RemoteSessionOptions {
            target,
            worker,
            mode,
            profile,
            resources,
            destination_sha,
            authorization_verifier,
            task_clone,
            runtime_config_source,
        } = options;
        if !matches!(&target.location, TargetLocation::Remote { .. }) {
            return Err(incompatible());
        }
        if let Some(task_clone) = task_clone {
            let runtime_config_source = runtime_config_source.ok_or_else(|| {
                ExecutionError::new(
                    ExecutionErrorKind::PolicyDenied,
                    "Remote execution requires the source checkout to verify private project configuration",
                    false,
                )
            })?;
            crate::harness::runtime_config::ensure_remote_transfer_allowed(
                task_clone,
                Some(runtime_config_source),
            )
            .map_err(|_| {
                ExecutionError::new(
                    ExecutionErrorKind::PolicyDenied,
                    "Remote execution is blocked because local private project configuration has no remote worker approval",
                    false,
                )
            })?;
        }
        let verifier = authorization_verifier.as_ref().ok_or_else(|| {
            ExecutionError::new(
                ExecutionErrorKind::PolicyDenied,
                "Remote execution requires an active task claim",
                false,
            )
        })?;
        verifier.verify(&verifier.generation())?;
        let backend = Arc::new(match task_clone {
            Some(path) => {
                let runtime_config_source = runtime_config_source.ok_or_else(|| {
                    ExecutionError::new(
                        ExecutionErrorKind::PolicyDenied,
                        "Remote execution requires the source checkout to verify private project configuration",
                        false,
                    )
                })?;
                RemoteExecutionBackend::connect_with_workspace(
                    worker,
                    target.clone(),
                    path,
                    runtime_config_source,
                )?
            }
            None => RemoteExecutionBackend::connect(worker, target.clone())?,
        });
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|_| incompatible())?;
        let probe = runtime.block_on(backend.probe(&target))?;
        if !probe.available || !probe.compatible {
            return Err(ExecutionError::new(
                ExecutionErrorKind::EnvironmentUnavailable,
                "Remote Docker worker does not meet the execution requirements",
                true,
            ));
        }
        let architecture = probe.architecture.as_deref().ok_or_else(incompatible)?;
        let image = ImageLock::bundled()
            .map_err(|_| incompatible())?
            .select(profile, architecture)
            .map_err(|failure| {
                ExecutionError::new(
                    ExecutionErrorKind::ImageUnavailable,
                    failure.message,
                    failure.retryable,
                )
            })?;
        let capabilities = probe.capabilities.as_ref().ok_or_else(incompatible)?;
        if !capabilities.trusted_image_digests.contains(&image.digest) {
            return Err(ExecutionError::new(
                ExecutionErrorKind::ImageUnavailable,
                "Remote worker does not approve the selected executor image digest",
                false,
            ));
        }
        let resources = resources.restricted_by(&capabilities.resource_ceiling);
        let TargetLocation::Remote { worker_id } = &target.location else {
            return Err(incompatible());
        };
        let attempt_id = uuid::Uuid::new_v4().hyphenated().to_string();
        verifier.begin_remote_attempt(&attempt_id, worker_id)?;
        Self::new_with_destination_authorization(
            backend,
            workspace,
            target,
            mode,
            image.digest,
            resources,
            destination_sha,
            authorization_verifier,
        )
        .map_err(|_| incompatible())
    }
}

fn incompatible() -> ExecutionError {
    ExecutionError::new(
        ExecutionErrorKind::WorkerIncompatible,
        "Remote worker capability negotiation is incompatible",
        false,
    )
}

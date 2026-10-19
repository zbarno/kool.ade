//! Local execution backend. Repository paths enter only through trusted resolvers.

use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{Arc, Mutex},
};

use crate::execution::{
    backend::ExecutionHandleV1,
    contract::WorkspaceRef,
    docker::{
        engine::{DockerEndpoint, EngineDiscovery},
        images::SelectedImage,
        sandbox::{SandboxIdentity, ValidatedWorkspace},
    },
    errors::{ExecutionError, ExecutionErrorKind},
    session::AuthorizationGenerationVerifier,
};
use bollard::Docker;

mod admission;
#[cfg(test)]
mod diagnostics;
mod lifecycle;
mod prepared_image;
mod qa;
mod starting_guard;
mod system_image;
#[cfg(test)]
#[path = "backend/tests.rs"]
mod tests;
mod workspace_resolver;
use lifecycle::RuntimeExecution;
pub(super) use prepared_image::PreparedDerivedImage;
pub use workspace_resolver::{OwnedTaskWorkspaceResolver, ReadOnlyProjectWorkspaceResolver};

/// Resolves a `WorkspaceRef` only to an app-owned task clone and provides a verified staging root.
pub trait TaskWorkspaceResolver: Send + Sync {
    fn resolve_owned_clone(&self, workspace: &WorkspaceRef) -> Result<PathBuf, ExecutionError>;
    fn staging_root(&self) -> PathBuf;

    /// Return a separately staged private-file directory for this exact run.
    /// Implementations must bind it to an active, unrevoked receipt matching
    /// the execution, workspace, enrolled worker identity, and expiry. A
    /// structurally sealed directory or execution ID alone grants no access.
    fn private_files_mount_root(
        &self,
        _execution_id: &str,
    ) -> Result<Option<PathBuf>, ExecutionError> {
        Ok(None)
    }
}

pub struct DockerExecutionBackend {
    pub(super) docker: Docker,
    engine: Arc<EngineDiscovery>,
    resolver: Arc<dyn TaskWorkspaceResolver>,
    identity: SandboxIdentity,
    authorization_verifier: Option<Arc<dyn AuthorizationGenerationVerifier>>,
    workspaces: Mutex<HashMap<String, Arc<Mutex<PreparedWorkspace>>>>,
    executions: Mutex<HashMap<String, Arc<Mutex<RuntimeExecution>>>>,
    starting: Mutex<HashMap<String, (u32, u64)>>,
    dependency_caches: Mutex<
        HashMap<
            crate::execution::contract::DependencyCacheRefV1,
            crate::harness::resource_bridge::CacheSnapshot,
        >,
    >,
    derived_images: Mutex<HashMap<String, system_image::RegisteredDerivedImage>>,
}

impl DockerExecutionBackend {
    pub fn connect(
        endpoint: DockerEndpoint,
        resolver: Arc<dyn TaskWorkspaceResolver>,
        owner_instance_id: String,
    ) -> Result<Self, ExecutionError> {
        Self::connect_with_authorization(endpoint, resolver, owner_instance_id, None)
    }

    pub fn connect_with_authorization(
        endpoint: DockerEndpoint,
        resolver: Arc<dyn TaskWorkspaceResolver>,
        owner_instance_id: String,
        authorization_verifier: Option<Arc<dyn AuthorizationGenerationVerifier>>,
    ) -> Result<Self, ExecutionError> {
        let docker = endpoint.connect().map_err(prerequisite_error)?;
        let engine = EngineDiscovery::connect(endpoint.clone()).map_err(prerequisite_error)?;
        Ok(Self {
            docker,
            engine: Arc::new(engine),
            resolver,
            identity: SandboxIdentity {
                owner_instance_id,
                policy_version: "docker-sandbox-v1".into(),
                lease_expires_unix_ms: now_unix_ms().saturating_add(30 * 60 * 1000),
            },
            authorization_verifier,
            workspaces: Mutex::new(HashMap::new()),
            executions: Mutex::new(HashMap::new()),
            starting: Mutex::new(HashMap::new()),
            dependency_caches: Mutex::new(HashMap::new()),
            derived_images: Mutex::new(HashMap::new()),
        })
    }

    pub(super) fn verify_authorization(&self, generation: &str) -> Result<(), ExecutionError> {
        if let Some(verifier) = &self.authorization_verifier {
            if verifier.generation() != generation {
                return Err(ExecutionError::new(
                    ExecutionErrorKind::PolicyDenied,
                    "Execution authorization generation does not match its app-owned claim",
                    false,
                ));
            }
            verifier.verify(generation)?;
        }
        Ok(())
    }

    fn lookup_execution(
        &self,
        handle: &ExecutionHandleV1,
    ) -> Result<Arc<Mutex<RuntimeExecution>>, ExecutionError> {
        let execution = self
            .executions
            .lock()
            .map_err(|_| internal_error_with_context("reading the execution registry"))?
            .get(&handle.execution_id)
            .cloned()
            .ok_or_else(|| {
                ExecutionError::new(
                    ExecutionErrorKind::WorkerDisconnected,
                    "Execution is no longer owned by this application instance",
                    false,
                )
            })?;
        let state = execution
            .lock()
            .map_err(|_| internal_error_with_context("reading an execution record"))?;
        if !handle_matches(
            handle,
            &state.request.execution_id,
            &state.request.task_id,
            &state.request.target,
            state.request.request_sequence,
        ) {
            return Err(ExecutionError::new(
                ExecutionErrorKind::PolicyDenied,
                "Execution handle does not match its task, target, and request sequence",
                false,
            ));
        }
        drop(state);
        Ok(execution)
    }
}

fn handle_matches(
    handle: &ExecutionHandleV1,
    execution_id: &str,
    task_id: &str,
    target: &crate::execution::contract::ExecutionTarget,
    request_sequence: u64,
) -> bool {
    handle.execution_id == execution_id
        && handle.task_id == task_id
        && &handle.target == target
        && handle.request_sequence == request_sequence
}

pub(super) struct PreparedWorkspace {
    pub workspace: WorkspaceRef,
    pub expires_unix_ms: u64,
    pub verified: ValidatedWorkspace,
    pub starting: bool,
    pub active_execution_id: Option<String>,
}

fn prerequisite_error(
    failure: crate::execution::docker::discovery::PrerequisiteFailure,
) -> ExecutionError {
    ExecutionError::new(
        ExecutionErrorKind::EnvironmentUnavailable,
        failure.message,
        failure.retryable,
    )
}

pub(super) fn internal_error_with_context(context: &'static str) -> ExecutionError {
    ExecutionError::new(
        ExecutionErrorKind::WorkerDisconnected,
        format!("Execution state could not be accessed while {context}"),
        true,
    )
}

pub(super) fn internal_error_with_detail(context: &'static str, detail: &str) -> ExecutionError {
    let detail = detail
        .chars()
        .filter(|character| !character.is_control())
        .take(512)
        .collect::<String>();
    ExecutionError::new(
        ExecutionErrorKind::WorkerDisconnected,
        format!("Execution state could not be accessed while {context}: {detail}"),
        true,
    )
}

fn invalid_request() -> ExecutionError {
    ExecutionError::new(
        ExecutionErrorKind::PolicyDenied,
        "Execution request failed policy validation",
        false,
    )
}

fn now_unix_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .min(u64::MAX as u128) as u64
}

pub(super) fn selected_image(
    backend: &DockerExecutionBackend,
    digest: &str,
    architecture: &str,
) -> Result<SelectedImage, ExecutionError> {
    if let Some(image) = backend
        .derived_images
        .lock()
        .map_err(|_| internal_error_with_context("reading derived system images"))?
        .get(digest)
        .map(|registered| registered.image.clone())
    {
        let expected = crate::execution::docker::discovery::parse_architecture(architecture)
            .ok_or_else(|| internal_error_with_context("validating derived image architecture"))?;
        if image.digest == digest && image.platform == format!("linux/{expected}") {
            return Ok(image);
        }
        return Err(ExecutionError::new(
            ExecutionErrorKind::ImageUnavailable,
            "Derived image digest is not approved for this target architecture",
            false,
        ));
    }
    let lock = super::images::ImageLock::bundled().map_err(prerequisite_error)?;
    for profile in [
        super::images::RuntimeProfile::General,
        super::images::RuntimeProfile::Rust,
        super::images::RuntimeProfile::Node,
        super::images::RuntimeProfile::Python,
        super::images::RuntimeProfile::Go,
    ] {
        let candidate = lock
            .select(profile, architecture)
            .map_err(prerequisite_error)?;
        if candidate.digest == digest {
            return Ok(candidate);
        }
    }
    Err(ExecutionError::new(
        ExecutionErrorKind::ImageUnavailable,
        "Requested image digest is not in the approved executor manifest",
        false,
    ))
}

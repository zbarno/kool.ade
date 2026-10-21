use std::{
    collections::BTreeSet,
    sync::{
        Arc, Mutex, RwLock,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    time::Duration,
};

use crate::execution::{
    backend::{ExecutionBackend, ExecutionHandleV1},
    contract::{
        DependencyCacheRefV1, ExecutionModeV1, ExecutionRequestV1, ExecutionResultV1,
        ExecutionTarget, ResourceBudgetV1, WorkspaceRef,
    },
    errors::{ExecutionError, ExecutionErrorKind},
    policy::ExecutionPolicyV1,
};

mod commands;
mod constructors;
mod dependency_cache;
mod local;
mod process;
mod remote;
mod remote_dependency;
mod retry;
mod run;
mod system_image;

mod authorization;
pub use authorization::AuthorizationGenerationVerifier;
pub(crate) use retry::is_confirmed_remote_worker_loss;

#[derive(Debug)]
pub struct ExecutionOutput {
    pub stdout: String,
    pub stderr: String,
    pub result: ExecutionResultV1,
}

pub(super) struct RequestParameters {
    pub argv: Vec<String>,
    pub stdin: Option<String>,
    pub working_directory_relative: String,
    pub environment_allowlist: BTreeSet<String>,
    pub timeout: Duration,
    pub request_sequence: u64,
    pub cancellation_generation: u64,
    pub policy_override: Option<ExecutionPolicyV1>,
    pub artifact_paths: Vec<String>,
}

/// Controller-owned session that turns tool scripts into bounded backend requests.
pub struct ExecutionSession {
    backend: Arc<dyn ExecutionBackend>,
    workspace: WorkspaceRef,
    destination_sha: Option<String>,
    target: ExecutionTarget,
    mode: ExecutionModeV1,
    image_digest: RwLock<String>,
    derived_image: Mutex<Option<crate::execution::docker::image_extension::DerivedImage>>,
    authorization_generation: String,
    authorization_verifier: Option<Arc<dyn AuthorizationGenerationVerifier>>,
    remote_retry_factory: Mutex<Option<Arc<RemoteRetryFactory>>>,
    retry_destination: Mutex<Option<Arc<ExecutionSession>>>,
    resources: ResourceBudgetV1,
    dependency_cache_refs: Mutex<Vec<DependencyCacheRefV1>>,
    prepared_dependency_ids: Mutex<BTreeSet<String>>,
    remote_dependency_requests: Mutex<
        std::collections::BTreeMap<
            String,
            crate::execution::remote::dependency::DependencyRelayRequestV1,
        >,
    >,
    sequence: AtomicU64,
    cancellation_generation: AtomicU64,
    busy: AtomicBool,
    cancel_requested: AtomicBool,
    active: Mutex<Option<(ExecutionHandleV1, u64)>>,
    remote_capacity_lease:
        Mutex<Option<crate::persistence::remote_workers::RemoteWorkerCapacityLease>>,
    runtime_processes: Mutex<Vec<Arc<process::RuntimeProcessHandle>>>,
    runtime: tokio::runtime::Runtime,
}

pub(crate) type RemoteRetryFactory =
    dyn Fn() -> Result<Arc<ExecutionSession>, ExecutionError> + Send + Sync;

pub use process::RuntimeProcessHandle;
pub use remote::RemoteSessionOptions;

impl ExecutionSession {
    #[cfg(test)]
    pub fn terminate_remote_connection_for_test(&self) {
        self.backend.terminate_remote_connection_for_test();
    }

    #[cfg(test)]
    pub fn active_handle_for_test(&self) -> Option<crate::execution::backend::ExecutionHandleV1> {
        self.active
            .lock()
            .ok()?
            .as_ref()
            .map(|(handle, _)| handle.clone())
    }

    pub fn workspace(&self) -> &WorkspaceRef {
        &self.workspace
    }

    pub fn target_id(&self) -> &str {
        &self.target.target_id
    }

    pub fn authorization_generation(&self) -> &str {
        &self.authorization_generation
    }

    pub(crate) fn authorization_verifier(
        &self,
    ) -> Result<Arc<dyn AuthorizationGenerationVerifier>, ExecutionError> {
        self.verify_authorization()?;
        self.authorization_verifier
            .clone()
            .ok_or_else(|| policy_error("Remote dependency relay requires an active task claim"))
    }

    /// Execute a shell command submitted by the application-owned MCP service.
    /// Commands are wrapped as stdin scripts and remain subject to the mode's
    /// shared Docker policy and the session's workspace binding.
    pub fn run_tool_command(
        &self,
        command: &str,
        timeout: Duration,
    ) -> Result<ExecutionOutput, ExecutionError> {
        if command.trim().is_empty() {
            return Err(policy_error("Tool command cannot be empty"));
        }
        self.run_bounded_script(command, timeout)
    }

    pub fn cancel(&self) -> Result<(), ExecutionError> {
        if let Some(destination) = self.retry_destination()? {
            return destination.cancel();
        }
        if !self.busy.load(Ordering::Acquire) {
            self.cancellation_generation.fetch_add(1, Ordering::AcqRel);
            return self.stop_runtime_processes();
        }
        self.cancel_requested.store(true, Ordering::Release);
        self.cancellation_generation.fetch_add(1, Ordering::AcqRel);
        let active = self.active.lock().map_err(|_| internal_error())?.clone();
        let Some((handle, generation)) = active else {
            return Ok(());
        };
        self.runtime
            .block_on(self.backend.cancel(&handle, generation))
    }

    fn request(&self, parameters: RequestParameters) -> Result<ExecutionRequestV1, ExecutionError> {
        let RequestParameters {
            argv,
            stdin,
            working_directory_relative,
            environment_allowlist,
            timeout,
            request_sequence,
            cancellation_generation,
            policy_override,
            artifact_paths,
        } = parameters;
        let allowed_service_ports = BTreeSet::new();
        let mut policy = match policy_override {
            Some(policy) => policy,
            None => {
                let policy = ExecutionPolicyV1::for_mode(
                    self.mode,
                    self.resources.clone(),
                    allowed_service_ports.clone(),
                )
                .map_err(|_| policy_error("Execution mode does not permit this command"))?;
                // Scoped writes are only meaningful with the exact recipe
                // authorization supplied by run_scoped_recipe.
                policy.without_scoped_workspace_write()
            }
        };
        if !artifact_paths.is_empty() {
            policy = policy
                .with_artifact_paths(artifact_paths)
                .map_err(|_| policy_error("Requested artifact paths are outside policy limits"))?;
        }
        let request = ExecutionRequestV1 {
            schema_version: 1,
            execution_id: uuid::Uuid::new_v4().hyphenated().to_string(),
            authorization_generation: self.authorization_generation.clone(),
            project_id: self.workspace.project_id.clone(),
            task_id: self.workspace.task_id.clone(),
            target: self.target.clone(),
            controller_target_id: None,
            workspace: self.workspace.clone(),
            destination_sha: self.destination_sha.clone(),
            mode: self.mode,
            image_digest: self.selected_image_digest()?,
            argv,
            working_directory_relative,
            stdin,
            environment_allowlist,
            resource_budget: self.resources.clone(),
            deadline_unix_ms: now_unix_ms()
                .saturating_add(timeout.as_millis().min(u64::MAX as u128) as u64),
            allowed_service_ports,
            broker_capability_ids: BTreeSet::new(),
            dependency_cache_refs: self
                .dependency_cache_refs
                .lock()
                .map_err(|_| internal_error())?
                .clone(),
            cancellation_generation,
            request_sequence,
            policy,
        };
        request.validate_at(now_unix_ms()).map_err(|error| {
            policy_error(&format!("Execution request failed validation: {error:#}"))
        })?;
        Ok(request)
    }

    fn clear_active(&self) {
        if let Ok(mut active) = self.active.lock() {
            *active = None;
        }
        self.busy.store(false, Ordering::Release);
        self.cancel_requested.store(false, Ordering::Release);
    }

    fn ensure_not_cancelled(&self) -> Result<(), ExecutionError> {
        if self.cancel_requested.load(Ordering::Acquire) {
            Err(cancelled_error())
        } else {
            Ok(())
        }
    }
}

fn now_unix_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .min(u64::MAX as u128) as u64
}

fn make_runtime() -> std::io::Result<tokio::runtime::Runtime> {
    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
}

pub(super) fn current_platform() -> crate::execution::docker::engine::HostPlatform {
    use crate::execution::docker::engine::HostPlatform;
    #[cfg(target_os = "linux")]
    return HostPlatform::Linux;
    #[cfg(target_os = "macos")]
    return HostPlatform::MacOs;
    #[cfg(target_os = "windows")]
    return HostPlatform::Windows;
    #[allow(unreachable_code)]
    HostPlatform::Linux
}

fn policy_error(message: &str) -> ExecutionError {
    ExecutionError::new(ExecutionErrorKind::PolicyDenied, message, false)
}

fn internal_error() -> ExecutionError {
    ExecutionError::new(
        ExecutionErrorKind::WorkerDisconnected,
        "Execution session state could not be accessed",
        true,
    )
}

fn cancelled_error() -> ExecutionError {
    ExecutionError::new(
        ExecutionErrorKind::Cancelled,
        "Execution was cancelled before the command completed",
        false,
    )
}

#[cfg(test)]
#[path = "session/tests.rs"]
mod tests;

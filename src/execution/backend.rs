use crate::execution::{
    contract::{
        EventPageRequestV1, ExecutionEventV1, ExecutionRequestV1, ExecutionResultV1,
        ExecutionTarget, WorkspaceRef,
    },
    errors::ExecutionError,
};
use std::{future::Future, pin::Pin};

/// Opaque, controller-verified dependency cache archive offered to a backend.
/// Only the owning crate can inspect its manifest and payload.
#[derive(Clone, Debug)]
pub struct DependencyCacheSnapshot {
    pub(crate) snapshot: crate::harness::resource_bridge::CacheSnapshot,
}

fn unsupported_qa_driver() -> ExecutionError {
    ExecutionError::new(
        crate::execution::errors::ExecutionErrorKind::WorkerIncompatible,
        "Selected execution backend cannot manage Runtime QA web previews",
        false,
    )
}

pub type BackendFuture<'a, T> =
    Pin<Box<dyn Future<Output = Result<T, ExecutionError>> + Send + 'a>>;

/// Shared lifecycle API for local and remote Docker workers.
/// Implementations resolve local paths internally; request messages never carry them.
pub trait ExecutionBackend: Send + Sync {
    #[cfg(test)]
    fn terminate_remote_connection_for_test(&self) {}

    /// Derive a system image from a separately user-authorized package closure.
    /// Remote and unsupported backends reject this by default.
    #[allow(private_interfaces)]
    fn derive_authorized_system_image<'a>(
        &'a self,
        _request: &'a crate::execution::docker::image_extension::AuthorizedImageExtensionRequest,
        _packages: Vec<crate::execution::docker::image_extension::builder::VerifiedPackageArtifact>,
    ) -> BackendFuture<'a, crate::execution::docker::image_extension::DerivedImage> {
        Box::pin(async {
            Err(ExecutionError::new(
                crate::execution::errors::ExecutionErrorKind::WorkerIncompatible,
                "Selected execution target does not support authorized system image derivation",
                false,
            ))
        })
    }

    #[allow(private_interfaces)]
    fn inspect_system_package_state<'a>(
        &'a self,
        _base: &'a crate::execution::docker::image_extension::BaseImageSelection,
    ) -> BackendFuture<
        'a,
        std::collections::BTreeMap<String, crate::harness::resource_bridge::InstalledPackage>,
    > {
        Box::pin(async {
            Err(ExecutionError::new(
                crate::execution::errors::ExecutionErrorKind::WorkerIncompatible,
                "Selected execution target cannot inspect the approved base image package state",
                false,
            ))
        })
    }

    /// Launch a policy-validated disposable Runtime QA driver on this backend's
    /// already-selected engine. Backends without a local Docker control plane
    /// fail closed by default.
    fn create_qa_driver<'a>(
        &'a self,
        _spec: &'a crate::execution::docker::qa_container::QaDriverContainerSpec<'a>,
        _uploads: &'a [crate::execution::docker::qa_container::QaContainerUpload],
        _cancel: &'a mut tokio::sync::watch::Receiver<bool>,
    ) -> BackendFuture<'a, String> {
        Box::pin(async {
            Err(ExecutionError::new(
                crate::execution::errors::ExecutionErrorKind::WorkerIncompatible,
                "Selected execution backend cannot launch isolated Runtime QA drivers",
                false,
            ))
        })
    }

    fn run_qa_driver<'a>(
        &'a self,
        _container_id: &'a str,
        _spec: &'a crate::execution::docker::qa_container::QaDriverContainerSpec<'a>,
        _cancel: &'a mut tokio::sync::watch::Receiver<bool>,
    ) -> BackendFuture<'a, crate::execution::docker::qa_container::DriverContainerOutput> {
        Box::pin(async {
            Err(ExecutionError::new(
                crate::execution::errors::ExecutionErrorKind::WorkerIncompatible,
                "Selected execution backend cannot launch isolated Runtime QA drivers",
                false,
            ))
        })
    }

    fn cleanup_qa_driver<'a>(&'a self, _container_id: &'a str) -> BackendFuture<'a, ()> {
        Box::pin(async {
            Err(ExecutionError::new(
                crate::execution::errors::ExecutionErrorKind::WorkerIncompatible,
                "Selected execution backend cannot clean up Runtime QA drivers",
                false,
            ))
        })
    }

    fn start_qa_web_preview<'a>(
        &'a self,
        _spec: crate::execution::docker::qa_container::QaWebPreviewSpec<'a>,
        _cancel: &'a mut tokio::sync::watch::Receiver<bool>,
    ) -> BackendFuture<'a, crate::execution::docker::qa_container::QaWebPreview> {
        Box::pin(async { Err(unsupported_qa_driver()) })
    }

    fn cleanup_qa_web_preview<'a>(
        &'a self,
        _lease: &'a crate::runtime_qa::QaTaskServiceLeaseV1,
        _binding: &'a crate::runtime_qa::QaPreviewRelayBindingV1,
    ) -> BackendFuture<'a, ()> {
        Box::pin(async { Err(unsupported_qa_driver()) })
    }

    fn recover_qa_web_preview<'a>(
        &'a self,
        _request: &'a crate::runtime_qa::QaRunRequestV1,
    ) -> BackendFuture<'a, ()> {
        Box::pin(async { Err(unsupported_qa_driver()) })
    }

    /// Stage a controller-verified cache snapshot on this worker. The reference
    /// is opaque; adapters must not resolve host paths from it.
    fn stage_dependency_cache<'a>(
        &'a self,
        _snapshot: DependencyCacheSnapshot,
    ) -> BackendFuture<'a, ()> {
        Box::pin(async {
            Err(ExecutionError::new(
                crate::execution::errors::ExecutionErrorKind::WorkerIncompatible,
                "Execution target cannot receive dependency cache snapshots",
                false,
            ))
        })
    }

    /// Ask an authenticated remote worker to emit an identity-bound package
    /// request before the controller's broker evaluates it.
    fn relay_dependency_request<'a>(
        &'a self,
        _request: crate::execution::remote::dependency::DependencyRelayRequestV1,
    ) -> BackendFuture<'a, crate::execution::remote::dependency::DependencyRelayRequestV1> {
        Box::pin(async {
            Err(ExecutionError::new(
                crate::execution::errors::ExecutionErrorKind::WorkerIncompatible,
                "Selected execution target cannot relay dependency requests",
                false,
            ))
        })
    }

    /// Return the controller broker's decision to the worker. This conveys no
    /// credentials or cache bytes; prepared cache data uses the separate
    /// integrity checked transfer above.
    fn relay_dependency_decision<'a>(
        &'a self,
        _response: crate::execution::remote::dependency::DependencyRelayResponseV1,
    ) -> BackendFuture<'a, ()> {
        Box::pin(async {
            Err(ExecutionError::new(
                crate::execution::errors::ExecutionErrorKind::WorkerIncompatible,
                "Selected execution target cannot return dependency decisions",
                false,
            ))
        })
    }

    fn probe(&self, target: &ExecutionTarget) -> BackendFuture<'_, BackendProbeV1>;

    fn prepare_workspace<'a>(
        &'a self,
        workspace: &'a WorkspaceRef,
        target: &'a ExecutionTarget,
    ) -> BackendFuture<'a, WorkspaceLeaseV1>;

    fn start<'a>(
        &'a self,
        lease: &'a WorkspaceLeaseV1,
        request: &'a ExecutionRequestV1,
    ) -> BackendFuture<'a, ExecutionHandleV1>;

    fn release_workspace<'a>(&'a self, lease: &'a WorkspaceLeaseV1) -> BackendFuture<'a, ()>;

    fn exec<'a>(
        &'a self,
        handle: &'a ExecutionHandleV1,
        request: &'a ExecutionRequestV1,
    ) -> BackendFuture<'a, ()>;

    fn events<'a>(
        &'a self,
        handle: &'a ExecutionHandleV1,
        page: EventPageRequestV1,
    ) -> BackendFuture<'a, Vec<ExecutionEventV1>>;

    fn cancel<'a>(
        &'a self,
        handle: &'a ExecutionHandleV1,
        cancellation_generation: u64,
    ) -> BackendFuture<'a, ()>;

    fn collect_artifacts<'a>(
        &'a self,
        handle: &'a ExecutionHandleV1,
    ) -> BackendFuture<'a, ExecutionResultV1>;

    /// Apply a verified remote workspace result through the controller's
    /// app-owned task-clone synchronization boundary. Local backends have
    /// already synchronized during execution and keep the default no-op.
    fn apply_workspace_result<'a>(
        &'a self,
        _handle: &'a ExecutionHandleV1,
        _request: &'a ExecutionRequestV1,
    ) -> BackendFuture<'a, ()> {
        Box::pin(async { Ok(()) })
    }

    fn release<'a>(&'a self, handle: &'a ExecutionHandleV1) -> BackendFuture<'a, ()>;

    /// Release a completed persistent runtime and its workspace reservation.
    /// Backends whose `release` also owns workspace cleanup may override this.
    fn release_persistent<'a>(
        &'a self,
        handle: &'a ExecutionHandleV1,
        lease: &'a WorkspaceLeaseV1,
    ) -> BackendFuture<'a, ()> {
        Box::pin(async move {
            self.release(handle).await?;
            self.release_workspace(lease).await
        })
    }
}

#[derive(Clone, Debug, serde::Deserialize, serde::Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct BackendProbeV1 {
    pub wire_version: u16,
    pub available: bool,
    pub compatible: bool,
    #[serde(default)]
    pub architecture: Option<String>,
    pub failure: Option<crate::execution::errors::ExecutionErrorKind>,
    #[serde(default)]
    pub capabilities: Option<BackendCapabilitiesV1>,
}

/// Negotiated worker/runtime facts. These values are observations, not grants;
/// every request is still checked against the local immutable policy.
#[derive(Clone, Debug, serde::Deserialize, serde::Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct BackendCapabilitiesV1 {
    pub app_version: String,
    pub docker_version: Option<String>,
    pub docker_api_version: Option<String>,
    pub docker_rootless: Option<bool>,
    pub docker_desktop: Option<bool>,
    pub linux_containers: Option<bool>,
    pub seccomp_enabled: Option<bool>,
    pub memory_limit: Option<bool>,
    pub cpu_quota: Option<bool>,
    pub pids_limit: Option<bool>,
    pub disk_available_bytes: Option<u64>,
    pub current_execution_count: u32,
    #[serde(default)]
    pub maximum_execution_count: Option<u32>,
    pub resource_ceiling: crate::execution::contract::ResourceBudgetV1,
    pub trusted_image_digests: Vec<String>,
    pub policy_hash: String,
}

#[derive(Clone, Debug, serde::Deserialize, serde::Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct WorkspaceLeaseV1 {
    pub lease_id: String,
    pub workspace: WorkspaceRef,
    pub expires_unix_ms: u64,
}

#[derive(Clone, Debug, serde::Deserialize, serde::Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ExecutionHandleV1 {
    pub execution_id: String,
    pub task_id: String,
    pub target: ExecutionTarget,
    pub request_sequence: u64,
}

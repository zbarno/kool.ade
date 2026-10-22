use crate::execution::{
    backend::{
        BackendFuture, BackendProbeV1, ExecutionBackend, ExecutionHandleV1, WorkspaceLeaseV1,
    },
    contract::{
        EventPageRequestV1, ExecutionEventV1, ExecutionRequestV1, ExecutionResultV1,
        ExecutionTarget, WorkspaceRef,
    },
    docker::backend::{DockerExecutionBackend, internal_error_with_context as state_error},
    errors::ExecutionError,
};

mod cache;
mod control;
mod execute;
mod qa_network;
mod recovery;
mod runtime;
mod workspace;
#[cfg(test)]
pub(in crate::execution::docker::backend) use execute::{
    inject_crash_after_stage_out, observe_cancel_race_archive, pause_after_stage_out,
    pause_before_collecting,
};
pub(crate) use qa_network::validate_qa_driver_network;
#[cfg(test)]
pub(in crate::execution::docker::backend::lifecycle) use recovery::reconcile_missing_container_404;
pub(super) use runtime::RuntimeExecution;
#[cfg(test)]
pub(in crate::execution::docker::backend::lifecycle) use workspace::{
    persist_runtime_wait_error, record_start_registration_failure,
};

impl ExecutionBackend for DockerExecutionBackend {
    #[allow(private_interfaces)]
    fn inspect_system_package_state<'a>(
        &'a self,
        base: &'a crate::execution::docker::image_extension::BaseImageSelection,
    ) -> BackendFuture<
        'a,
        std::collections::BTreeMap<String, crate::harness::resource_bridge::InstalledPackage>,
    > {
        Box::pin(self.inspect_system_packages(base))
    }

    #[allow(private_interfaces)]
    fn derive_authorized_system_image<'a>(
        &'a self,
        request: &'a crate::execution::docker::image_extension::AuthorizedImageExtensionRequest,
        packages: Vec<crate::execution::docker::image_extension::builder::VerifiedPackageArtifact>,
    ) -> BackendFuture<'a, crate::execution::docker::image_extension::DerivedImage> {
        Box::pin(self.derive_system_image(request, packages))
    }

    fn start_qa_web_preview<'a>(
        &'a self,
        spec: crate::execution::docker::qa_container::QaWebPreviewSpec<'a>,
        cancel: &'a mut tokio::sync::watch::Receiver<bool>,
    ) -> BackendFuture<'a, crate::execution::docker::qa_container::QaWebPreview> {
        Box::pin(async move { super::qa::start(self, spec, cancel).await })
    }

    fn cleanup_qa_web_preview<'a>(
        &'a self,
        lease: &'a crate::runtime_qa::QaTaskServiceLeaseV1,
        binding: &'a crate::runtime_qa::QaPreviewRelayBindingV1,
    ) -> BackendFuture<'a, ()> {
        Box::pin(async move { super::qa::cleanup(self, lease, binding).await })
    }

    fn recover_qa_web_preview<'a>(
        &'a self,
        request: &'a crate::runtime_qa::QaRunRequestV1,
    ) -> BackendFuture<'a, ()> {
        Box::pin(async move { super::qa::recover(self, request).await })
    }

    fn create_qa_driver<'a>(
        &'a self,
        spec: &'a crate::execution::docker::qa_container::QaDriverContainerSpec<'a>,
        uploads: &'a [crate::execution::docker::qa_container::QaContainerUpload],
        cancel: &'a mut tokio::sync::watch::Receiver<bool>,
    ) -> BackendFuture<'a, String> {
        Box::pin(async move {
            if *cancel.borrow() {
                return Err(state_error("checking QA driver cancellation"));
            }
            if let (Some(lease), Some(binding)) = (spec.service_lease, spec.relay_binding) {
                crate::execution::docker::qa_container::validate_preview_relay_network(
                    &self.docker,
                    spec.request,
                    lease,
                    binding,
                )
                .await?;
            }
            let body = spec.create_body()?;
            let name = format!("koolade-qa-{}", spec.request.qa_run_id);
            crate::execution::docker::qa_container::recover_driver_container(
                &self.docker,
                &name,
                spec.request,
                spec.owner_id,
                spec.driver,
            )
            .await?;
            crate::execution::docker::qa_container::create_driver_container(
                &self.docker,
                &name,
                body,
                uploads,
                std::time::Duration::from_secs(u64::from(
                    spec.request.bounded_limits.timeout_seconds,
                )),
                spec.request
                    .bounded_limits
                    .max_evidence_bytes
                    .min(64 * 1024 * 1024) as usize,
                cancel,
            )
            .await
        })
    }

    fn run_qa_driver<'a>(
        &'a self,
        container_id: &'a str,
        spec: &'a crate::execution::docker::qa_container::QaDriverContainerSpec<'a>,
        cancel: &'a mut tokio::sync::watch::Receiver<bool>,
    ) -> BackendFuture<'a, crate::execution::docker::qa_container::DriverContainerOutput> {
        Box::pin(async move {
            match (&spec.network, spec.service_lease, spec.relay_binding) {
                (crate::execution::docker::qa_container::QaDriverNetwork::Disabled, None, None) => {
                }
                (
                    crate::execution::docker::qa_container::QaDriverNetwork::InternalPreviewRelay {
                        ..
                    },
                    Some(lease),
                    Some(binding),
                ) => {
                    crate::execution::docker::qa_container::validate_preview_relay_network(
                        &self.docker,
                        spec.request,
                        lease,
                        binding,
                    )
                    .await?;
                    validate_qa_driver_network(
                        &self.docker,
                        spec.driver,
                        spec.request,
                        binding,
                        container_id,
                    )
                    .await?;
                }
                _ => return Err(state_error("validating the QA driver network binding")),
            }
            crate::execution::docker::qa_container::run_created_driver(
                &self.docker,
                container_id,
                std::time::Duration::from_secs(u64::from(
                    spec.request.bounded_limits.timeout_seconds,
                )),
                spec.request
                    .bounded_limits
                    .max_evidence_bytes
                    .min(64 * 1024 * 1024) as usize,
                cancel,
            )
            .await
        })
    }

    fn cleanup_qa_driver<'a>(&'a self, container_id: &'a str) -> BackendFuture<'a, ()> {
        Box::pin(async move {
            crate::execution::docker::qa_container::cleanup_driver_container(
                &self.docker,
                container_id,
            )
            .await
        })
    }

    fn stage_dependency_cache<'a>(
        &'a self,
        snapshot: crate::execution::backend::DependencyCacheSnapshot,
    ) -> BackendFuture<'a, ()> {
        cache::stage_dependency_cache(self, snapshot)
    }

    fn probe(&self, target: &ExecutionTarget) -> BackendFuture<'_, BackendProbeV1> {
        let target = target.clone();
        Box::pin(async move { workspace::probe(self, &target).await })
    }

    fn prepare_workspace<'a>(
        &'a self,
        workspace_ref: &'a WorkspaceRef,
        target: &'a ExecutionTarget,
    ) -> BackendFuture<'a, WorkspaceLeaseV1> {
        Box::pin(async move { workspace::prepare(self, workspace_ref, target).await })
    }

    fn start<'a>(
        &'a self,
        lease: &'a WorkspaceLeaseV1,
        request: &'a ExecutionRequestV1,
    ) -> BackendFuture<'a, ExecutionHandleV1> {
        Box::pin(async move { workspace::start(self, lease, request).await })
    }

    fn set_lifecycle_state(
        &self,
        handle: &ExecutionHandleV1,
        state: crate::execution::contract::ExecutionLifecycleV1,
    ) -> Result<(), ExecutionError> {
        control::set_lifecycle_state(self, handle, state)
    }

    fn release_workspace<'a>(&'a self, lease: &'a WorkspaceLeaseV1) -> BackendFuture<'a, ()> {
        workspace::release_workspace(self, lease)
    }

    fn exec<'a>(
        &'a self,
        handle: &'a ExecutionHandleV1,
        request: &'a ExecutionRequestV1,
    ) -> BackendFuture<'a, ()> {
        Box::pin(async move {
            let result = execute::run(self, handle, request).await;
            if let Err(error) = &result
                && let Ok(execution) = self.lookup_execution(handle)
            {
                let lifecycle = match error.kind {
                    crate::execution::errors::ExecutionErrorKind::WorkerDisconnected
                    | crate::execution::errors::ExecutionErrorKind::WorkerIncompatible => {
                        crate::execution::contract::ExecutionLifecycleV1::Interrupted
                    }
                    crate::execution::errors::ExecutionErrorKind::Cancelled => {
                        crate::execution::contract::ExecutionLifecycleV1::Cancelled
                    }
                    _ => crate::execution::contract::ExecutionLifecycleV1::Failed,
                };
                control::terminal(
                    &execution,
                    lifecycle,
                    Some(error.clone()),
                    None,
                    error.kind == crate::execution::errors::ExecutionErrorKind::DeadlineExceeded,
                    error.kind == crate::execution::errors::ExecutionErrorKind::Cancelled,
                )?;
            }
            result
        })
    }

    fn events<'a>(
        &'a self,
        handle: &'a ExecutionHandleV1,
        page: EventPageRequestV1,
    ) -> BackendFuture<'a, Vec<ExecutionEventV1>> {
        Box::pin(async move { control::events(self, handle, page).await })
    }

    fn cancel<'a>(
        &'a self,
        handle: &'a ExecutionHandleV1,
        generation: u64,
    ) -> BackendFuture<'a, ()> {
        Box::pin(async move { control::cancel(self, handle, generation).await })
    }

    fn collect_artifacts<'a>(
        &'a self,
        handle: &'a ExecutionHandleV1,
    ) -> BackendFuture<'a, ExecutionResultV1> {
        Box::pin(async move { control::collect(self, handle).await })
    }

    fn release<'a>(&'a self, handle: &'a ExecutionHandleV1) -> BackendFuture<'a, ()> {
        Box::pin(async move { control::release(self, handle).await })
    }

    fn release_persistent<'a>(
        &'a self,
        handle: &'a ExecutionHandleV1,
        _lease: &'a WorkspaceLeaseV1,
    ) -> BackendFuture<'a, ()> {
        self.release(handle)
    }
}

fn disconnected() -> ExecutionError {
    ExecutionError::new(
        crate::execution::errors::ExecutionErrorKind::WorkerDisconnected,
        "Docker Engine operation failed or disconnected",
        true,
    )
}

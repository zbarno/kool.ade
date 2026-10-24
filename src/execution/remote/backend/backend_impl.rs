use super::*;

mod cache;
mod dependency;
mod release;
mod results;
mod start;
mod system_image;
use crate::execution::{
    backend::{
        BackendFuture, BackendProbeV1, ExecutionBackend, ExecutionHandleV1, WorkspaceLeaseV1,
    },
    contract::{EventPageRequestV1, ExecutionEventV1, ExecutionResultV1},
};

impl ExecutionBackend for RemoteExecutionBackend {
    fn discard_interrupted_result<'a>(
        &'a self,
        handle: &'a ExecutionHandleV1,
    ) -> BackendFuture<'a, ()> {
        results::discard_interrupted_result(self, handle)
    }

    #[allow(private_interfaces)]
    fn inspect_system_package_state<'a>(
        &'a self,
        base: &'a crate::execution::docker::image_extension::BaseImageSelection,
    ) -> BackendFuture<
        'a,
        std::collections::BTreeMap<String, crate::harness::resource_bridge::InstalledPackage>,
    > {
        system_image::inspect(self, base)
    }

    #[allow(private_interfaces)]
    fn derive_authorized_system_image<'a>(
        &'a self,
        request: &'a crate::execution::docker::image_extension::AuthorizedImageExtensionRequest,
        packages: Vec<crate::execution::docker::image_extension::builder::VerifiedPackageArtifact>,
    ) -> BackendFuture<'a, crate::execution::docker::image_extension::DerivedImage> {
        system_image::derive(self, request, packages)
    }

    fn stage_dependency_cache<'a>(
        &'a self,
        snapshot: crate::execution::backend::DependencyCacheSnapshot,
    ) -> BackendFuture<'a, ()> {
        cache::stage(self, snapshot)
    }

    #[cfg(test)]
    fn terminate_remote_connection_for_test(&self) {
        if let Ok(mut client) = self.client.lock() {
            client.terminate_transport_for_test();
        }
    }

    fn relay_dependency_request<'a>(
        &'a self,
        request: crate::execution::remote::dependency::DependencyRelayRequestV1,
    ) -> BackendFuture<'a, crate::execution::remote::dependency::DependencyRelayRequestV1> {
        dependency::relay_request(self, request)
    }

    fn relay_dependency_decision<'a>(
        &'a self,
        response: crate::execution::remote::dependency::DependencyRelayResponseV1,
    ) -> BackendFuture<'a, ()> {
        dependency::relay_decision(self, response)
    }

    fn probe(&self, target: &ExecutionTarget) -> BackendFuture<'_, BackendProbeV1> {
        let target_check = self.require_target(target);
        Box::pin(async move {
            target_check?;
            let mut client = self.client.lock().map_err(|_| disconnected())?;
            client.refresh_probe_now()?;
            Ok(client.probe().clone())
        })
    }

    fn prepare_workspace<'a>(
        &'a self,
        workspace: &'a WorkspaceRef,
        target: &'a ExecutionTarget,
    ) -> BackendFuture<'a, WorkspaceLeaseV1> {
        Box::pin(async move {
            self.require_target(target)?;
            let (bytes, ms) = self.stage_workspace(workspace)?;
            let lease: WorkspaceLeaseV1 = self.request(
                EnvelopeKindV1::PrepareWorkspace,
                CONTROL_EXECUTION_ID,
                serde_json::json!({"workspace": workspace, "target": ExecutionTarget::local()}),
            )?;
            self.pending_workspace_transfers
                .lock()
                .map_err(|_| disconnected())?
                .insert(lease.lease_id.clone(), (bytes, ms));
            Ok(lease)
        })
    }

    fn start<'a>(
        &'a self,
        lease: &'a WorkspaceLeaseV1,
        request: &'a ExecutionRequestV1,
    ) -> BackendFuture<'a, ExecutionHandleV1> {
        start::start(self, lease, request)
    }

    fn release_workspace<'a>(&'a self, lease: &'a WorkspaceLeaseV1) -> BackendFuture<'a, ()> {
        release::release_workspace(self, lease)
    }

    fn exec<'a>(
        &'a self,
        handle: &'a ExecutionHandleV1,
        request: &'a ExecutionRequestV1,
    ) -> BackendFuture<'a, ()> {
        Box::pin(async move {
            self.stop_if_private_grant_revoked(handle).await?;
            let local_handle = self.local_handle(handle)?;
            self.require_target(&request.target)?;
            if handle.execution_id != request.execution_id {
                return Err(incompatible());
            }
            let mut worker_request = request.clone();
            worker_request.controller_target_id = Some(self.target.target_id.clone());
            worker_request.target = ExecutionTarget::local();
            let _: serde_json::Value = self.request(
                EnvelopeKindV1::Exec,
                &handle.execution_id,
                serde_json::json!({"handle": local_handle, "request": worker_request}),
            )?;
            Ok(())
        })
    }

    fn events<'a>(
        &'a self,
        handle: &'a ExecutionHandleV1,
        page: EventPageRequestV1,
    ) -> BackendFuture<'a, Vec<ExecutionEventV1>> {
        Box::pin(async move {
            self.stop_if_private_grant_revoked(handle).await?;
            let local_handle = self.local_handle(handle)?;
            let events = self.request(
                EnvelopeKindV1::Subscribe,
                &handle.execution_id,
                serde_json::json!({"handle": local_handle, "page": page}),
            )?;
            self.stop_if_private_grant_revoked(handle).await?;
            Ok(events)
        })
    }

    fn cancel<'a>(
        &'a self,
        handle: &'a ExecutionHandleV1,
        cancellation_generation: u64,
    ) -> BackendFuture<'a, ()> {
        Box::pin(async move {
            let local_handle = self.local_handle(handle)?;
            let _: serde_json::Value = self.request(
                EnvelopeKindV1::Cancel,
                &handle.execution_id,
                serde_json::json!({
                    "handle": local_handle,
                    "cancellation_generation": cancellation_generation,
                }),
            )?;
            Ok(())
        })
    }

    fn collect_artifacts<'a>(
        &'a self,
        handle: &'a ExecutionHandleV1,
    ) -> BackendFuture<'a, ExecutionResultV1> {
        results::collect_artifacts(self, handle)
    }

    fn apply_workspace_result<'a>(
        &'a self,
        handle: &'a ExecutionHandleV1,
        request: &'a ExecutionRequestV1,
    ) -> BackendFuture<'a, ()> {
        results::apply_workspace_result(self, handle, request)
    }

    fn release<'a>(&'a self, handle: &'a ExecutionHandleV1) -> BackendFuture<'a, ()> {
        release::release_execution(self, handle)
    }

    fn release_persistent<'a>(
        &'a self,
        handle: &'a ExecutionHandleV1,
        _lease: &'a WorkspaceLeaseV1,
    ) -> BackendFuture<'a, ()> {
        self.release(handle)
    }
}

use super::*;

mod cache;
mod dependency;
mod release;
mod results;
mod system_image;
use crate::execution::{
    backend::{
        BackendFuture, BackendProbeV1, ExecutionBackend, ExecutionHandleV1, WorkspaceLeaseV1,
    },
    contract::{EventPageRequestV1, ExecutionEventV1, ExecutionResultV1, TargetLocation},
};

impl ExecutionBackend for RemoteExecutionBackend {
    fn discard_interrupted_result<'a>(
        &'a self,
        handle: &'a ExecutionHandleV1,
    ) -> BackendFuture<'a, ()> {
        Box::pin(async move {
            let result = self
                .verified_results
                .lock()
                .map_err(|_| disconnected())?
                .get(&handle.execution_id)
                .cloned();
            if let Some(result) = result {
                private_files::remove_result_transfer(&self.result_root, result.transfer_id())?;
                self.verified_results
                    .lock()
                    .map_err(|_| disconnected())?
                    .remove(&handle.execution_id);
            }
            Ok(())
        })
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
        Box::pin(async move {
            self.require_target(&request.target)?;
            if lease.workspace != request.workspace {
                return Err(incompatible());
            }
            let mut worker_request = request.clone();
            worker_request.controller_target_id = Some(self.target.target_id.clone());
            worker_request.target = ExecutionTarget::local();
            let snapshot = self
                .snapshots
                .lock()
                .map_err(|_| disconnected())?
                .get(&request.workspace)
                .cloned()
                .ok_or_else(incompatible)?;
            let mut client = self.client.lock().map_err(|_| disconnected())?;
            let private_grants =
                private_files::stage_for_execution(self, &mut client, request, &snapshot)?;
            let grant_use = crate::persistence::remote_private_file_grants::lock_authorized_grants(
                &crate::persistence::state_root(),
                &private_grants,
                crate::persistence::harness_settings::current_unix_ms(),
            );
            let Ok(grant_use) = grant_use else {
                let _ = client.request(
                    EnvelopeKindV1::AbortPrivateFileStage,
                    &request.execution_id,
                    serde_json::json!({"execution_id": request.execution_id}),
                );
                return Err(private_files::revoked_error());
            };
            let execution_started_at = std::time::Instant::now();
            let response = client.request(
                EnvelopeKindV1::Start,
                &request.execution_id,
                serde_json::json!({"lease": lease, "request": worker_request}),
            )?;
            if let TargetLocation::Remote { worker_id } = &self.target.location {
                let _ = crate::persistence::remote_execution_telemetry::record_execution_started(
                    &request.execution_id,
                    &request.task_id,
                    worker_id,
                    &request.image_digest,
                    execution_started_at
                        .elapsed()
                        .as_millis()
                        .min(u64::MAX as u128) as u64,
                );
            }
            drop(grant_use);
            let upload = self
                .pending_workspace_transfers
                .lock()
                .ok()
                .and_then(|mut pending| pending.remove(&lease.lease_id));
            if let (Some((bytes, ms)), TargetLocation::Remote { worker_id }) =
                (upload, &self.target.location)
            {
                let _ = crate::persistence::remote_execution_telemetry::record_workspace_upload(
                    &request.execution_id,
                    &request.task_id,
                    worker_id,
                    bytes,
                    ms,
                );
            }
            let mut handle: ExecutionHandleV1 =
                serde_json::from_value(response.payload).map_err(|_| incompatible())?;
            handle.target = self.target.clone();
            drop(client);
            self.execution_snapshots
                .lock()
                .map_err(|_| disconnected())?
                .insert(handle.execution_id.clone(), snapshot);
            self.execution_started_at
                .lock()
                .map_err(|_| disconnected())?
                .insert(handle.execution_id.clone(), execution_started_at);
            if !private_grants.is_empty() {
                self.execution_private_file_grants
                    .lock()
                    .map_err(|_| disconnected())?
                    .insert(
                        handle.execution_id.clone(),
                        (private_grants, request.cancellation_generation),
                    );
            }
            Ok(handle)
        })
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
        Box::pin(async move {
            let local_handle = self.local_handle(handle)?;
            let _: serde_json::Value = self.request(
                EnvelopeKindV1::Release,
                &handle.execution_id,
                serde_json::json!({"handle": local_handle}),
            )?;
            self.execution_private_file_grants
                .lock()
                .map_err(|_| disconnected())?
                .remove(&handle.execution_id);
            Ok(())
        })
    }

    fn release_persistent<'a>(
        &'a self,
        handle: &'a ExecutionHandleV1,
        _lease: &'a WorkspaceLeaseV1,
    ) -> BackendFuture<'a, ()> {
        self.release(handle)
    }
}

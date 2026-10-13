use super::*;

mod cache;
mod release;
use crate::execution::{
    backend::{BackendFuture, BackendProbeV1, ExecutionBackend, WorkspaceLeaseV1},
    contract::{EventPageRequestV1, ExecutionEventV1, ExecutionResultV1},
};

impl ExecutionBackend for RemoteExecutionBackend {
    fn relay_dependency_request<'a>(
        &'a self,
        request: crate::execution::remote::dependency::DependencyRelayRequestV1,
    ) -> BackendFuture<'a, crate::execution::remote::dependency::DependencyRelayRequestV1> {
        Box::pin(async move {
            request.validate().map_err(|_| incompatible())?;
            if request.worker_id != self.target.target_id
                || request.workspace.task_id != request.task_id
            {
                return Err(incompatible());
            }
            let mut client = self.client.lock().map_err(|_| disconnected())?;
            let envelope = client.request(
                EnvelopeKindV1::RelayDependency,
                &request.execution_id,
                serde_json::to_value(&request).map_err(|_| incompatible())?,
            )?;
            let emitted: crate::execution::remote::dependency::DependencyRelayRequestV1 =
                serde_json::from_value(envelope.payload).map_err(|_| incompatible())?;
            if emitted != request {
                return Err(incompatible());
            }
            Ok(emitted)
        })
    }

    fn relay_dependency_decision<'a>(
        &'a self,
        response: crate::execution::remote::dependency::DependencyRelayResponseV1,
    ) -> BackendFuture<'a, ()> {
        Box::pin(async move {
            if response.worker_id != self.target.target_id {
                return Err(incompatible());
            }
            let _: serde_json::Value = self.request(
                EnvelopeKindV1::DependencyDecision,
                &response.execution_id,
                serde_json::to_value(&response).map_err(|_| incompatible())?,
            )?;
            Ok(())
        })
    }

    fn stage_dependency_cache<'a>(
        &'a self,
        snapshot: crate::execution::backend::DependencyCacheSnapshot,
    ) -> BackendFuture<'a, ()> {
        cache::stage(self, snapshot)
    }

    fn probe(&self, target: &ExecutionTarget) -> BackendFuture<'_, BackendProbeV1> {
        let target_check = self.require_target(target);
        Box::pin(async move {
            target_check?;
            Ok(self
                .client
                .lock()
                .map_err(|_| disconnected())?
                .probe()
                .clone())
        })
    }

    fn prepare_workspace<'a>(
        &'a self,
        workspace: &'a WorkspaceRef,
        target: &'a ExecutionTarget,
    ) -> BackendFuture<'a, WorkspaceLeaseV1> {
        Box::pin(async move {
            self.require_target(target)?;
            self.stage_workspace(workspace)?;
            self.request(
                EnvelopeKindV1::PrepareWorkspace,
                CONTROL_EXECUTION_ID,
                serde_json::json!({"workspace": workspace, "target": ExecutionTarget::local()}),
            )
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
            let mut handle: ExecutionHandleV1 = self.request(
                EnvelopeKindV1::Start,
                &request.execution_id,
                serde_json::json!({"lease": lease, "request": worker_request}),
            )?;
            handle.target = self.target.clone();
            let snapshot = self
                .snapshots
                .lock()
                .map_err(|_| disconnected())?
                .get(&request.workspace)
                .cloned()
                .ok_or_else(incompatible)?;
            self.execution_snapshots
                .lock()
                .map_err(|_| disconnected())?
                .insert(handle.execution_id.clone(), snapshot);
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
            let local_handle = self.local_handle(handle)?;
            self.request(
                EnvelopeKindV1::Subscribe,
                &handle.execution_id,
                serde_json::json!({"handle": local_handle, "page": page}),
            )
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
        Box::pin(async move {
            let local_handle = self.local_handle(handle)?;
            let result: ExecutionResultV1 = self.request(
                EnvelopeKindV1::CollectArtifacts,
                &handle.execution_id,
                serde_json::json!({"handle": local_handle}),
            )?;
            let workspace = self
                .execution_snapshots
                .lock()
                .map_err(|_| disconnected())?
                .get(&handle.execution_id)
                .cloned()
                .ok_or_else(incompatible)?;
            let (worker_id, _) = match &self.target.location {
                TargetLocation::Remote { worker_id } => (worker_id.clone(), ()),
                TargetLocation::Local => return Err(incompatible()),
            };
            let transfer: TransferManifestV1 = self.request(
                EnvelopeKindV1::PrepareWorkspaceResult,
                &handle.execution_id,
                serde_json::json!({"handle": local_handle, "workspace": workspace.workspace,
                    "snapshot_generation": workspace.snapshot_generation,
                    "destination_identity": format!("controller:{}", self.controller_id)}),
            )?;
            let mut receiver = StagingReceiver::begin(
                &self.result_root,
                transfer.clone(),
                &format!("worker:{worker_id}"),
                &format!("controller:{}", self.controller_id),
            )
            .map_err(|_| incompatible())?;
            for (path, size_bytes) in transfer.file_specs() {
                let mut offset = 0_u64;
                while offset < size_bytes {
                    let chunk: crate::execution::remote::staging::TransferChunkV1 = self.request(
                        EnvelopeKindV1::ReadWorkspaceResultChunk,
                        &handle.execution_id,
                        serde_json::json!({"handle": local_handle, "workspace": workspace.workspace,
                            "transfer_id": transfer.transfer_id, "path": path, "offset": offset}),
                    )?;
                    let advanced = chunk.bytes.len() as u64;
                    receiver.accept_chunk(&chunk).map_err(|_| incompatible())?;
                    offset = offset.checked_add(advanced).ok_or_else(incompatible)?;
                    if advanced == 0 {
                        return Err(incompatible());
                    }
                }
            }
            let verified = receiver
                .finalize_workspace_result(&workspace.workspace)
                .map_err(|_| incompatible())?;
            self.verified_results
                .lock()
                .map_err(|_| disconnected())?
                .insert(handle.execution_id.clone(), verified);
            Ok(result)
        })
    }

    fn apply_workspace_result<'a>(
        &'a self,
        handle: &'a ExecutionHandleV1,
        request: &'a ExecutionRequestV1,
    ) -> BackendFuture<'a, ()> {
        Box::pin(async move {
            let result = self
                .verified_results
                .lock()
                .map_err(|_| disconnected())?
                .remove(&handle.execution_id)
                .ok_or_else(incompatible)?;
            if result.workspace() != &request.workspace {
                return Err(incompatible());
            }
            let baseline = self
                .baselines
                .lock()
                .map_err(|_| disconnected())?
                .get(&request.workspace)
                .cloned()
                .ok_or_else(incompatible)?;
            let archive = apply::result_to_archive(&result)?;
            staging::synchronize(
                &baseline.0,
                &baseline.1,
                &archive,
                request.resource_budget.temporary_disk_bytes,
            )
        })
    }

    fn release<'a>(&'a self, handle: &'a ExecutionHandleV1) -> BackendFuture<'a, ()> {
        Box::pin(async move {
            let local_handle = self.local_handle(handle)?;
            let _: serde_json::Value = self.request(
                EnvelopeKindV1::Release,
                &handle.execution_id,
                serde_json::json!({"handle": local_handle}),
            )?;
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

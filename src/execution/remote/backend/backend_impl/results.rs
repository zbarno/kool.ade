use super::super::{RemoteExecutionBackend, apply, disconnected, incompatible, private_files};
use crate::execution::{
    backend::{BackendFuture, ExecutionHandleV1},
    contract::{ExecutionRequestV1, ExecutionResultV1, TargetLocation},
    remote::{
        protocol::EnvelopeKindV1,
        staging::{StagingReceiver, TransferManifestV1},
    },
};

pub(super) fn collect_artifacts<'a>(
    backend: &'a RemoteExecutionBackend,
    handle: &'a ExecutionHandleV1,
) -> BackendFuture<'a, ExecutionResultV1> {
    Box::pin(async move {
        backend.stop_if_private_grant_revoked(handle).await?;
        let local_handle = backend.local_handle(handle)?;
        let result: ExecutionResultV1 = backend.request(
            EnvelopeKindV1::CollectArtifacts,
            &handle.execution_id,
            serde_json::json!({"handle": local_handle}),
        )?;
        let workspace = backend
            .execution_snapshots
            .lock()
            .map_err(|_| disconnected())?
            .get(&handle.execution_id)
            .cloned()
            .ok_or_else(incompatible)?;
        let execution_wall_ms = backend
            .execution_started_at
            .lock()
            .ok()
            .and_then(|started| started.get(&handle.execution_id).copied())
            .map(|started| started.elapsed().as_millis().min(u64::MAX as u128) as u64);
        let telemetry_result =
            crate::persistence::remote_execution_telemetry::record_execution_result(
                &handle.execution_id,
                &workspace.workspace.task_id,
                match &backend.target.location {
                    TargetLocation::Remote { worker_id } => worker_id,
                    TargetLocation::Local => return Err(incompatible()),
                },
                execution_wall_ms,
                result.metrics.cpu_millis,
                result.metrics.peak_memory_bytes,
                result.failure.as_ref().map(|failure| failure.kind),
            );
        if execution_wall_ms.is_some()
            && telemetry_result.is_ok()
            && let Ok(mut started) = backend.execution_started_at.lock()
        {
            started.remove(&handle.execution_id);
        }
        backend.stop_if_private_grant_revoked(handle).await?;
        let worker_id = match &backend.target.location {
            TargetLocation::Remote { worker_id } => worker_id.clone(),
            TargetLocation::Local => return Err(incompatible()),
        };
        let transfer_started = std::time::Instant::now();
        let transfer: TransferManifestV1 = backend.request(
            EnvelopeKindV1::PrepareWorkspaceResult,
            &handle.execution_id,
            serde_json::json!({"handle": local_handle, "workspace": workspace.workspace,
                "snapshot_generation": workspace.snapshot_generation,
                "destination_identity": format!("controller:{}", backend.controller_id)}),
        )?;
        let mut receiver = StagingReceiver::begin(
            &backend.result_root,
            transfer.clone(),
            &format!("worker:{worker_id}"),
            &format!("controller:{}", backend.controller_id),
        )
        .map_err(|_| incompatible())?;
        let mut transferred_bytes = 0_u64;
        for (path, size_bytes) in transfer.file_specs() {
            let mut offset = 0_u64;
            while offset < size_bytes {
                if let Err(error) = backend.stop_if_private_grant_revoked(handle).await {
                    private_files::remove_result_transfer(
                        &backend.result_root,
                        &transfer.transfer_id,
                    )?;
                    return Err(error);
                }
                let chunk: crate::execution::remote::staging::TransferChunkV1 = backend.request(
                    EnvelopeKindV1::ReadWorkspaceResultChunk,
                    &handle.execution_id,
                    serde_json::json!({"handle": local_handle, "workspace": workspace.workspace,
                        "transfer_id": transfer.transfer_id, "path": path, "offset": offset}),
                )?;
                let advanced = chunk.bytes.len() as u64;
                transferred_bytes = transferred_bytes.saturating_add(advanced);
                receiver.accept_chunk(&chunk).map_err(|_| incompatible())?;
                offset = offset.checked_add(advanced).ok_or_else(incompatible)?;
                if advanced == 0 {
                    return Err(incompatible());
                }
            }
        }
        if let Err(error) = backend.stop_if_private_grant_revoked(handle).await {
            private_files::remove_result_transfer(&backend.result_root, &transfer.transfer_id)?;
            return Err(error);
        }
        let verified = receiver
            .finalize_workspace_result(&workspace.workspace)
            .map_err(|_| incompatible())?;
        let elapsed_ms = transfer_started.elapsed().as_millis().min(u64::MAX as u128) as u64;
        let _ = crate::persistence::remote_execution_telemetry::record_result_download(
            &handle.execution_id,
            &workspace.workspace.task_id,
            &worker_id,
            transferred_bytes,
            elapsed_ms,
        );
        if let Err(error) = backend.stop_if_private_grant_revoked(handle).await {
            private_files::remove_result_transfer(&backend.result_root, &transfer.transfer_id)?;
            return Err(error);
        }
        backend
            .verified_results
            .lock()
            .map_err(|_| disconnected())?
            .insert(handle.execution_id.clone(), verified);
        Ok(result)
    })
}

pub(super) fn apply_workspace_result<'a>(
    backend: &'a RemoteExecutionBackend,
    handle: &'a ExecutionHandleV1,
    request: &'a ExecutionRequestV1,
) -> BackendFuture<'a, ()> {
    Box::pin(async move {
        backend.stop_if_private_grant_revoked(handle).await?;
        let result = backend
            .verified_results
            .lock()
            .map_err(|_| disconnected())?
            .get(&handle.execution_id)
            .cloned()
            .ok_or_else(incompatible)?;
        if result.workspace() != &request.workspace {
            return Err(incompatible());
        }
        let baseline = backend
            .baselines
            .lock()
            .map_err(|_| disconnected())?
            .get(&request.workspace)
            .cloned()
            .ok_or_else(incompatible)?;
        backend.stop_if_private_grant_revoked(handle).await?;
        let archive = apply::result_to_archive(&result)?;
        // The grant-store lock makes the final authorization check atomic with
        // workspace synchronization: revocation cannot commit mid-apply.
        let _grant_use = backend.lock_execution_private_file_grants(&handle.execution_id)?;
        crate::execution::docker::sandbox::staging::synchronize(
            &baseline.0,
            &baseline.1,
            &archive,
            request.resource_budget.temporary_disk_bytes,
        )?;
        backend
            .verified_results
            .lock()
            .map_err(|_| disconnected())?
            .remove(&handle.execution_id);
        Ok(())
    })
}

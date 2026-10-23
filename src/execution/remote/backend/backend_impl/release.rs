use super::*;

pub(super) fn release_workspace<'a>(
    backend: &'a RemoteExecutionBackend,
    lease: &'a WorkspaceLeaseV1,
) -> BackendFuture<'a, ()> {
    Box::pin(async move {
        if let Ok(mut pending) = backend.pending_workspace_transfers.lock() {
            pending.remove(&lease.lease_id);
        }
        let _: serde_json::Value = backend.request(
            EnvelopeKindV1::ReleaseWorkspace,
            CONTROL_EXECUTION_ID,
            serde_json::json!({"lease": lease}),
        )?;
        Ok(())
    })
}

pub(super) fn release_execution<'a>(
    backend: &'a RemoteExecutionBackend,
    handle: &'a ExecutionHandleV1,
) -> BackendFuture<'a, ()> {
    Box::pin(async move {
        let local_handle = backend.local_handle(handle)?;
        let response: serde_json::Value = backend.request(
            EnvelopeKindV1::Release,
            &handle.execution_id,
            serde_json::json!({"handle": local_handle}),
        )?;
        if let (TargetLocation::Remote { worker_id }, Some(encoded_timings)) =
            (&backend.target.location, response.get("phase_timings"))
            && let Ok(phase_timings) = serde_json::from_value::<
                crate::execution::contract::ExecutionPhaseTimingsV1,
            >(encoded_timings.clone())
        {
            let _ = crate::persistence::remote_execution_telemetry::record_execution_phase_timings(
                &handle.execution_id,
                &handle.task_id,
                worker_id,
                &phase_timings,
            );
        }
        backend
            .execution_private_file_grants
            .lock()
            .map_err(|_| disconnected())?
            .remove(&handle.execution_id);
        Ok(())
    })
}

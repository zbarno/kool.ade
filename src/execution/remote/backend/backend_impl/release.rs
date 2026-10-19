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

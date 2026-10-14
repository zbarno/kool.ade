use super::*;

pub(super) fn release_workspace<'a>(
    backend: &'a RemoteExecutionBackend,
    lease: &'a WorkspaceLeaseV1,
) -> BackendFuture<'a, ()> {
    Box::pin(async move {
        let _: serde_json::Value = backend.request(
            EnvelopeKindV1::ReleaseWorkspace,
            CONTROL_EXECUTION_ID,
            serde_json::json!({"lease": lease}),
        )?;
        Ok(())
    })
}

use super::*;

pub(super) fn relay_request<'a>(
    backend: &'a RemoteExecutionBackend,
    request: crate::execution::remote::dependency::DependencyRelayRequestV1,
) -> BackendFuture<'a, crate::execution::remote::dependency::DependencyRelayRequestV1> {
    Box::pin(async move {
        request.validate().map_err(|_| incompatible())?;
        if request.worker_id != backend.target.target_id
            || request.workspace.task_id != request.task_id
        {
            return Err(incompatible());
        }
        let mut client = backend.client.lock().map_err(|_| disconnected())?;
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

pub(super) fn relay_decision<'a>(
    backend: &'a RemoteExecutionBackend,
    response: crate::execution::remote::dependency::DependencyRelayResponseV1,
) -> BackendFuture<'a, ()> {
    Box::pin(async move {
        if response.worker_id != backend.target.target_id {
            return Err(incompatible());
        }
        let _: serde_json::Value = backend.request(
            EnvelopeKindV1::DependencyDecision,
            &response.execution_id,
            serde_json::to_value(&response).map_err(|_| incompatible())?,
        )?;
        Ok(())
    })
}

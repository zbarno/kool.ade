use super::*;

pub(super) fn dispatch(
    backend: Arc<dyn ExecutionBackend>,
    stager: Option<Arc<crate::execution::remote::staging::WorkerStager>>,
    runtime: &tokio::runtime::Runtime,
    envelope: &EnvelopeV1,
) -> Result<(Kind, Value), ExecutionError> {
    match envelope.kind {
        Kind::PrepareWorkspace => {
            require_control_id(envelope)?;
            let request: PrepareRequest = decode(&envelope.payload)?;
            let reservation = stager
                .as_ref()
                .map(|stager| stager.reserve_workspace(&request.workspace))
                .transpose()
                .map_err(|_| {
                    super::super::incompatible("workspace generation could not be reserved")
                })?;
            let lease =
                runtime.block_on(backend.prepare_workspace(&request.workspace, &request.target))?;
            if let Some(reservation) = reservation {
                reservation.bind_lease(&lease.lease_id).map_err(|_| {
                    super::super::incompatible("workspace lease could not be reserved")
                })?;
            }
            json_value(lease).map(|value| (Kind::Acknowledge, value))
        }
        Kind::BeginWorkspaceTransfer => {
            require_control_id(envelope)?;
            let request: BeginWorkspaceTransferRequest = decode(&envelope.payload)?;
            stager
                .as_ref()
                .ok_or_else(|| {
                    super::super::incompatible("worker workspace staging is unavailable")
                })?
                .begin(request.manifest)
                .map_err(|_| {
                    super::super::incompatible("workspace transfer manifest was rejected")
                })?;
            Ok((Kind::Acknowledge, json!({"accepted": true})))
        }
        Kind::WorkspaceTransferChunk => {
            require_control_id(envelope)?;
            let request: WorkspaceTransferChunkRequest = decode(&envelope.payload)?;
            stager
                .as_ref()
                .ok_or_else(|| {
                    super::super::incompatible("worker workspace staging is unavailable")
                })?
                .accept_chunk(&request.chunk)
                .map_err(|_| super::super::incompatible("workspace transfer chunk was rejected"))?;
            Ok((Kind::Acknowledge, json!({"accepted": true})))
        }
        Kind::FinalizeWorkspaceTransfer => {
            require_control_id(envelope)?;
            let request: FinalizeWorkspaceTransferRequest = decode(&envelope.payload)?;
            stager
                .as_ref()
                .ok_or_else(|| {
                    super::super::incompatible("worker workspace staging is unavailable")
                })?
                .finalize(&request.transfer_id)
                .map_err(|_| {
                    super::super::incompatible("workspace transfer could not be finalized")
                })?;
            Ok((Kind::Acknowledge, json!({"accepted": true})))
        }
        Kind::ReleaseWorkspace => {
            require_control_id(envelope)?;
            let request: ReleaseWorkspaceRequest = decode(&envelope.payload)?;
            runtime.block_on(backend.release_workspace(&request.lease))?;
            if let Some(stager) = &stager {
                stager
                    .release_workspace(&request.lease.lease_id)
                    .map_err(|_| {
                        super::super::incompatible("workspace generation could not be released")
                    })?;
            }
            Ok((Kind::Acknowledge, json!({"released": true})))
        }
        _ => unreachable!(),
    }
}

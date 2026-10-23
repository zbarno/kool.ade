use crate::execution::{
    backend::{ExecutionHandleV1, WorkspaceLeaseV1},
    contract::{EventPageRequestV1, ExecutionRequestV1, ExecutionTarget, WorkspaceRef},
};
use serde::Deserialize;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ProbeRequest {
    pub(super) target: ExecutionTarget,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct PrepareRequest {
    pub(super) workspace: WorkspaceRef,
    pub(super) target: ExecutionTarget,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct BeginWorkspaceTransferRequest {
    pub(super) manifest: crate::execution::remote::staging::TransferManifestV1,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct WorkspaceTransferChunkRequest {
    pub(super) chunk: crate::execution::remote::staging::TransferChunkV1,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct FinalizeWorkspaceTransferRequest {
    pub(super) transfer_id: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct PrepareWorkspaceResultRequest {
    pub(super) handle: ExecutionHandleV1,
    pub(super) workspace: WorkspaceRef,
    pub(super) snapshot_generation: String,
    pub(super) destination_identity: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ReadWorkspaceResultChunkRequest {
    pub(super) handle: ExecutionHandleV1,
    pub(super) workspace: WorkspaceRef,
    pub(super) transfer_id: String,
    pub(super) path: String,
    pub(super) offset: u64,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct AuthorizePrivateFileGrantRequest {
    pub(super) execution_id: String,
    pub(super) workspace: crate::execution::contract::WorkspaceRef,
    pub(super) grant: crate::execution::remote::private_files::RemotePrivateFileGrantV1,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct AbortPrivateFileStageRequest {
    pub(super) execution_id: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct StartRequest {
    pub(super) lease: WorkspaceLeaseV1,
    pub(super) request: ExecutionRequestV1,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ExecRequest {
    pub(super) handle: ExecutionHandleV1,
    pub(super) request: ExecutionRequestV1,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct HandleRequest {
    pub(super) handle: ExecutionHandleV1,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct CancelRequest {
    pub(super) handle: ExecutionHandleV1,
    pub(super) cancellation_generation: u64,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct EventsRequest {
    pub(super) handle: ExecutionHandleV1,
    pub(super) page: EventPageRequestV1,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ReleaseWorkspaceRequest {
    pub(super) lease: WorkspaceLeaseV1,
}

#[derive(Deserialize, serde::Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct HeartbeatRequest {
    pub(super) nonce: String,
}

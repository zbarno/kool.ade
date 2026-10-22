use std::sync::Arc;

use serde_json::{Value, json};

use crate::execution::{
    backend::ExecutionBackend,
    docker::backend::TaskWorkspaceResolver,
    errors::ExecutionError,
    remote::{protocol::EnvelopeV1, staging::WorkerStager},
};

use crate::execution::remote::protocol::EnvelopeKindV1 as Kind;

use super::{
    requests::ExecRequest,
    support::{decode, require_execution_id},
};

pub(super) fn dispatch(
    backend: Arc<dyn ExecutionBackend>,
    stager: Option<Arc<WorkerStager>>,
    runtime: &tokio::runtime::Runtime,
    envelope: &EnvelopeV1,
) -> Result<(Kind, Value), ExecutionError> {
    let request: ExecRequest = decode(&envelope.payload)?;
    require_execution_id(envelope, &request.handle.execution_id)?;

    // The controller's request still says `false`; derive the effective value
    // from the worker's validated stage, matching the request saved at Start.
    let mut worker_request = request.request;
    worker_request.private_files_attached = if let Some(stager) = stager {
        stager
            .validate_private_file_workspace(
                &request.handle.execution_id,
                &worker_request.workspace,
            )
            .map_err(|_| {
                super::super::incompatible("private-file grant belongs to another workspace")
            })?;
        stager
            .private_files_mount_root(&request.handle.execution_id)
            .map_err(|_| {
                super::super::incompatible("worker private-file grant is incomplete or expired")
            })?
            .is_some()
    } else {
        false
    };

    runtime.spawn(async move {
        let _ = backend.exec(&request.handle, &worker_request).await;
    });
    Ok((Kind::Acknowledge, json!({"accepted": true})))
}

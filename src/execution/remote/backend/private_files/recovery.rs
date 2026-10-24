use std::collections::HashMap;

use crate::execution::{
    contract::ExecutionRequestV1, errors::ExecutionError,
    remote::private_files::RemotePrivateFileGrantV1,
};

use super::super::{RemoteExecutionBackend, disconnected};
use super::incompatible;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(in crate::execution::remote::backend) struct ActivePrivateFileExecution {
    pub(in crate::execution::remote::backend) task_id: String,
    pub(in crate::execution::remote::backend) request_sequence: u64,
    pub(in crate::execution::remote::backend) cancellation_generation: u64,
    pub(in crate::execution::remote::backend) grants: Vec<RemotePrivateFileGrantV1>,
    pub(in crate::execution::remote::backend) start_payload_sha256: String,
    pub(in crate::execution::remote::backend) stage_complete: bool,
    pub(in crate::execution::remote::backend) start_request_sequence: Option<u64>,
}

pub(in crate::execution::remote::backend) fn recover_execution_grants_at(
    root: &std::path::Path,
    worker_id: &str,
    controller_id: &str,
    host_key_fingerprint: &str,
) -> anyhow::Result<HashMap<String, ActivePrivateFileExecution>> {
    let mut recovered = HashMap::new();
    for record in
        crate::persistence::remote_private_file_executions::load_for_worker(root, worker_id)?
    {
        anyhow::ensure!(record.controller_id == controller_id);
        let mut grants = Vec::with_capacity(record.grant_ids.len());
        for grant_id in &record.grant_ids {
            let grant = crate::persistence::remote_private_file_grants::receipt_by_id(
                root,
                &record.project_id,
                grant_id,
            )?;
            anyhow::ensure!(
                grant.workspace.project_id == record.project_id
                    && grant.workspace.task_id == record.task_id
                    && grant.worker_id == record.worker_id
                    && grant.controller_id == record.controller_id
                    && grant.worker_host_key_fingerprint == host_key_fingerprint
            );
            grants.push(grant);
        }
        let active = ActivePrivateFileExecution {
            task_id: record.task_id,
            request_sequence: record.request_sequence,
            cancellation_generation: record.cancellation_generation,
            grants,
            start_payload_sha256: record.start_payload_sha256,
            stage_complete: record.stage_complete,
            start_request_sequence: record.start_request_sequence,
        };
        anyhow::ensure!(
            recovered.insert(record.execution_id, active).is_none(),
            "private file execution ID is duplicated"
        );
    }
    Ok(recovered)
}

pub(in crate::execution::remote::backend) fn active_execution(
    backend: &RemoteExecutionBackend,
    execution_id: &str,
) -> Result<Option<ActivePrivateFileExecution>, ExecutionError> {
    backend
        .execution_private_file_grants
        .lock()
        .map_err(|_| disconnected())
        .map(|executions| executions.get(execution_id).cloned())
}

pub(in crate::execution::remote::backend) fn save_execution_grants(
    root: &std::path::Path,
    request: &ExecutionRequestV1,
    worker_id: &str,
    controller_id: &str,
    grants: &[RemotePrivateFileGrantV1],
    start_payload_sha256: &str,
) -> Result<ActivePrivateFileExecution, ExecutionError> {
    let mut grant_ids = grants
        .iter()
        .map(|grant| grant.grant_id.clone())
        .collect::<Vec<_>>();
    grant_ids.sort();
    let record =
        crate::persistence::remote_private_file_executions::RemotePrivateFileExecutionGrantV1 {
            schema_version: 1,
            execution_id: request.execution_id.clone(),
            project_id: request.workspace.project_id.clone(),
            task_id: request.workspace.task_id.clone(),
            worker_id: worker_id.to_owned(),
            controller_id: controller_id.to_owned(),
            request_sequence: request.request_sequence,
            cancellation_generation: request.cancellation_generation,
            grant_ids,
            start_payload_sha256: start_payload_sha256.to_owned(),
            stage_complete: false,
            start_request_sequence: None,
        };
    crate::persistence::remote_private_file_executions::save(root, &record)
        .map_err(|_| disconnected())?;
    Ok(ActivePrivateFileExecution {
        task_id: request.task_id.clone(),
        request_sequence: request.request_sequence,
        cancellation_generation: request.cancellation_generation,
        grants: grants.to_vec(),
        start_payload_sha256: start_payload_sha256.to_owned(),
        stage_complete: false,
        start_request_sequence: None,
    })
}

pub(in crate::execution::remote::backend) fn mark_stage_complete(
    backend: &RemoteExecutionBackend,
    execution_id: &str,
) -> Result<(), ExecutionError> {
    let worker_id = worker_id(backend)?;
    crate::persistence::remote_private_file_executions::mark_stage_complete(
        &crate::persistence::state_root(),
        worker_id,
        execution_id,
    )
    .map_err(|_| disconnected())?;
    let mut executions = backend
        .execution_private_file_grants
        .lock()
        .map_err(|_| disconnected())?;
    let active = executions.get_mut(execution_id).ok_or_else(disconnected)?;
    active.stage_complete = true;
    Ok(())
}

pub(in crate::execution::remote::backend) fn bind_start_request(
    backend: &RemoteExecutionBackend,
    execution_id: &str,
    sequence: u64,
) -> Result<(), ExecutionError> {
    let worker_id = worker_id(backend)?;
    crate::persistence::remote_private_file_executions::bind_start_request(
        &crate::persistence::state_root(),
        worker_id,
        execution_id,
        sequence,
    )
    .map_err(|_| disconnected())?;
    let mut executions = backend
        .execution_private_file_grants
        .lock()
        .map_err(|_| disconnected())?;
    let active = executions.get_mut(execution_id).ok_or_else(disconnected)?;
    active.start_request_sequence = Some(sequence);
    Ok(())
}

fn worker_id(backend: &RemoteExecutionBackend) -> Result<&str, ExecutionError> {
    match &backend.target.location {
        crate::execution::contract::TargetLocation::Remote { worker_id } => Ok(worker_id),
        crate::execution::contract::TargetLocation::Local => Err(incompatible()),
    }
}

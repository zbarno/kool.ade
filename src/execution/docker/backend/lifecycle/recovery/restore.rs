use std::sync::{Arc, Mutex};

use bollard::query_parameters::InspectContainerOptionsBuilder;

use crate::execution::{
    backend::ExecutionHandleV1,
    docker::{
        backend::{DockerExecutionBackend, internal_error},
        sandbox::ValidatedWorkspace,
    },
    errors::{ExecutionError, ExecutionErrorKind},
};
use crate::persistence::execution_records::{self, ExecutionRecordV1};

use super::interrupt::mark_interrupted;
use super::verify_record_labels;

pub(super) async fn restore_record(
    backend: &DockerExecutionBackend,
    record: &mut ExecutionRecordV1,
    task_lock: std::fs::File,
) -> Result<(), ExecutionError> {
    let name = record
        .container_id
        .as_deref()
        .or(record.container_name.as_deref());
    let Some(name) = name else {
        mark_interrupted(record, true, "Execution container identity was lost")?;
        return Ok(());
    };
    let inspected = tokio::time::timeout(
        std::time::Duration::from_secs(5),
        backend.docker.inspect_container(
            name,
            Some(
                InspectContainerOptionsBuilder::default()
                    .size(false)
                    .build(),
            ),
        ),
    )
    .await
    .map_err(|_| recovery_error("Docker resume inspection timed out"))?;
    let info = match inspected {
        Ok(info) => info,
        Err(bollard::errors::Error::DockerResponseServerError {
            status_code: 404, ..
        }) => {
            mark_interrupted(record, true, "Execution container no longer exists")?;
            return Ok(());
        }
        Err(_) => return Err(recovery_error("Docker resume inspection failed")),
    };
    verify_record_labels(
        record,
        &info
            .config
            .as_ref()
            .and_then(|config| config.labels.clone())
            .unwrap_or_default(),
    )?;
    let request = record
        .request
        .clone()
        .ok_or_else(|| recovery_error("Execution record does not contain a resumable request"))?;
    let workspace_path = backend.resolver.resolve_owned_clone(&request.workspace)?;
    let workspace = ValidatedWorkspace::inspect(&workspace_path)?;
    let container_running = info.state.as_ref().and_then(|state| state.running) == Some(true);
    if !container_running && !is_terminal(record.state) {
        mark_interrupted(
            record,
            false,
            "Execution worker stopped before its result was saved",
        )?;
    }
    let restored =
        super::super::runtime::RuntimeExecution::restore(record, workspace.clone(), task_lock)?;
    let lease_id = record
        .workspace_lease_id
        .clone()
        .ok_or_else(|| recovery_error("Execution record does not contain a workspace lease"))?;
    let expires = record.lease_expires_at_unix_ms.ok_or_else(|| {
        recovery_error("Execution record does not contain a workspace lease expiry")
    })?;
    let prepared = Arc::new(Mutex::new(super::super::super::PreparedWorkspace {
        workspace: request.workspace.clone(),
        expires_unix_ms: expires,
        verified: workspace,
        starting: false,
        active_execution_id: Some(record.execution_id.clone()),
    }));
    backend
        .workspaces
        .lock()
        .map_err(|_| internal_error())?
        .insert(lease_id, prepared);
    let handle = ExecutionHandleV1 {
        execution_id: request.execution_id.clone(),
        task_id: request.task_id.clone(),
        target: request.target.clone(),
        request_sequence: request.request_sequence,
    };
    backend
        .executions
        .lock()
        .map_err(|_| internal_error())?
        .insert(handle.execution_id, Arc::new(Mutex::new(restored)));
    Ok(())
}

fn is_terminal(state: execution_records::ExecutionRecordStateV1) -> bool {
    matches!(
        state,
        execution_records::ExecutionRecordStateV1::Succeeded
            | execution_records::ExecutionRecordStateV1::Failed
            | execution_records::ExecutionRecordStateV1::Cancelled
            | execution_records::ExecutionRecordStateV1::Interrupted
    )
}

fn recovery_error(message: &str) -> ExecutionError {
    ExecutionError::new(ExecutionErrorKind::WorkerDisconnected, message, true)
}

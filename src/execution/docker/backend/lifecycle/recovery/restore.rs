use std::sync::{Arc, Mutex};

use bollard::query_parameters::InspectContainerOptionsBuilder;

use crate::execution::{
    backend::ExecutionHandleV1,
    docker::{
        backend::{DockerExecutionBackend, internal_error_with_context},
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
        if preserve_missing_runtime_wait(
            record.state,
            record.command_started,
            record.container_id.is_some(),
        ) {
            return Ok(());
        }
        mark_interrupted(record, false, None, "Execution container identity was lost")?;
        return Ok(());
    };
    let cleanup_started = std::time::Instant::now();
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
            reconcile_missing_container_404(
                record,
                cleanup_started
                    .elapsed()
                    .as_millis()
                    .min(u128::from(u64::MAX)) as u64,
            )?;
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
    // A restarted worker has no reader attached to an already-started Docker
    // exec. Reopening that record as Running would leave event subscribers
    // waiting forever and could expose a partial, unobserved result. Reconcile
    // it to Interrupted and remove only the container whose complete ownership
    // labels were just verified.
    if record.command_started && !is_terminal(record.state) {
        super::reconcile_record(backend, record, false).await?;
        return Ok(());
    }
    // The v1 writer recorded Running only after dependency setup and workspace
    // upload completed. Older records therefore have no stage-in timing but
    // are complete. New incomplete starts remain Preparing and are discarded.
    if should_reconcile_before_command(record.command_started, record.state) {
        super::reconcile_record(backend, record, false).await?;
        return Ok(());
    }
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
            None,
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
        .map_err(|_| internal_error_with_context("restoring the workspace registry entry"))?
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
        .map_err(|_| internal_error_with_context("restoring the execution registry entry"))?
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

fn is_waiting(state: execution_records::ExecutionRecordStateV1) -> bool {
    matches!(
        state,
        execution_records::ExecutionRecordStateV1::WaitingForDependencyApproval
            | execution_records::ExecutionRecordStateV1::WaitingForRuntime
    )
}

pub(super) fn preserve_missing_runtime_wait(
    state: execution_records::ExecutionRecordStateV1,
    command_started: bool,
    has_container_id: bool,
) -> bool {
    state == execution_records::ExecutionRecordStateV1::WaitingForRuntime
        && !command_started
        && !has_container_id
}

/// Apply the complete state reconciliation for the Docker inspect 404 branch.
/// A pre-command runtime wait has no container to recover and remains retryable.
pub(in crate::execution::docker::backend::lifecycle) fn reconcile_missing_container_404(
    record: &mut ExecutionRecordV1,
    cleanup_ms: u64,
) -> Result<bool, ExecutionError> {
    if preserve_missing_runtime_wait(
        record.state,
        record.command_started,
        record.container_id.is_some(),
    ) {
        return Ok(true);
    }
    mark_interrupted(
        record,
        true,
        Some(cleanup_ms),
        "Execution container no longer exists",
    )?;
    Ok(false)
}

fn should_reconcile_before_command(
    command_started: bool,
    state: execution_records::ExecutionRecordStateV1,
) -> bool {
    !command_started
        && !is_terminal(state)
        && state != execution_records::ExecutionRecordStateV1::Running
        && !is_waiting(state)
}

#[cfg(test)]
#[path = "restore/tests.rs"]
mod tests;

fn recovery_error(message: &str) -> ExecutionError {
    ExecutionError::new(ExecutionErrorKind::WorkerDisconnected, message, true)
}

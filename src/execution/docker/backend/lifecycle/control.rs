use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};
use std::time::Duration;

use bollard::query_parameters::RemoveContainerOptions;

use crate::execution::{
    backend::ExecutionHandleV1,
    contract::{ExecutionLifecycleV1, ExecutionResultV1},
    docker::backend::{DockerExecutionBackend, internal_error_with_context, invalid_request},
    errors::{ExecutionError, ExecutionErrorKind},
};

use super::runtime::RuntimeExecution;

pub(super) fn set_lifecycle_state(
    backend: &DockerExecutionBackend,
    handle: &ExecutionHandleV1,
    state: ExecutionLifecycleV1,
) -> Result<(), ExecutionError> {
    let execution = backend
        .executions
        .lock()
        .map_err(|_| internal_error_with_context("updating the execution lifecycle"))?
        .get(&handle.execution_id)
        .cloned()
        .ok_or_else(|| internal_error_with_context("finding the active execution lifecycle"))?;
    let mut execution = execution
        .lock()
        .map_err(|_| internal_error_with_context("locking the active execution lifecycle"))?;
    if execution.request.task_id != handle.task_id
        || execution.request.target != handle.target
        || execution.request.request_sequence != handle.request_sequence
    {
        return Err(internal_error_with_context(
            "validating the active execution lifecycle handle",
        ));
    }
    execution.set_lifecycle_if_changed(state)
}

mod cancel;
mod cancel_active;
pub(super) use cancel_active::cancel_active as cancel;
mod events;
mod helpers;
mod release;
pub(super) use cancel::cancel_persisted;
pub(super) use events::events;
pub(super) use helpers::{
    disconnected, operation_disconnected, operation_timeout, result, terminal, verify_owned,
};
use release::release_persisted;

struct CancellationIntentGuard(Arc<AtomicUsize>);

impl CancellationIntentGuard {
    fn new(requests: Arc<AtomicUsize>) -> Self {
        requests.fetch_add(1, Ordering::AcqRel);
        Self(requests)
    }
}

impl Drop for CancellationIntentGuard {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::AcqRel);
    }
}

fn load_record(
    backend: &DockerExecutionBackend,
    handle: &ExecutionHandleV1,
) -> Result<crate::persistence::execution_records::ExecutionRecordV1, ExecutionError> {
    let record = crate::persistence::execution_records::load(&handle.task_id, &handle.execution_id)
        .map_err(|_| internal_error_with_context("loading the durable execution record"))?
        .ok_or_else(disconnected)?;
    if record.owner_instance_id != backend.identity.owner_instance_id
        || record.target_id != handle.target.target_id
        || u64::from(record.attempt) != handle.request_sequence
    {
        return Err(ExecutionError::new(
            ExecutionErrorKind::PolicyDenied,
            "Execution record ownership does not match this handle",
            false,
        ));
    }
    Ok(record)
}

fn elapsed_millis(started: std::time::Instant) -> u64 {
    started.elapsed().as_millis().min(u128::from(u64::MAX)) as u64
}

pub(super) async fn collect(
    backend: &DockerExecutionBackend,
    handle: &ExecutionHandleV1,
) -> Result<ExecutionResultV1, ExecutionError> {
    let record = load_record(backend, handle)?;
    if record.result.as_ref().is_some_and(is_worker_loss_result)
        && !worker_loss_is_reconciled(record.state, record.cleanup)
    {
        return Err(ExecutionError::new(
            ExecutionErrorKind::WorkerDisconnected,
            "Worker execution is interrupted but its container stop is not confirmed",
            true,
        ));
    }
    let execution = match backend.lookup_execution(handle) {
        Ok(execution) => execution,
        Err(error) if error.kind == ExecutionErrorKind::WorkerDisconnected => {
            return record.result.ok_or_else(|| {
                ExecutionError::new(
                    ExecutionErrorKind::ExecutionFailed,
                    "Execution has no confirmed terminal result",
                    false,
                )
            });
        }
        Err(error) => return Err(error),
    };
    execution
        .lock()
        .map_err(|_| internal_error_with_context("reading the terminal execution result"))?
        .result
        .clone()
        .ok_or_else(|| {
            ExecutionError::new(
                ExecutionErrorKind::ExecutionFailed,
                "Execution has no confirmed terminal result",
                false,
            )
        })
}

fn is_worker_loss_result(result: &ExecutionResultV1) -> bool {
    result.exit_code.is_none()
        && result.signal.is_none()
        && !result.cancelled
        && !result.timed_out
        && result.failure.as_ref().is_some_and(|failure| {
            failure.kind == ExecutionErrorKind::WorkerDisconnected && failure.retryable
        })
}

fn worker_loss_is_reconciled(
    state: crate::persistence::execution_records::ExecutionRecordStateV1,
    cleanup: crate::persistence::execution_records::ExecutionCleanupStateV1,
) -> bool {
    state == crate::persistence::execution_records::ExecutionRecordStateV1::Interrupted
        && cleanup == crate::persistence::execution_records::ExecutionCleanupStateV1::Complete
}

pub(super) async fn release(
    backend: &DockerExecutionBackend,
    handle: &ExecutionHandleV1,
) -> Result<(), ExecutionError> {
    let execution = match backend.lookup_execution(handle) {
        Ok(execution) => execution,
        Err(error) if error.kind == ExecutionErrorKind::WorkerDisconnected => {
            return release_persisted(backend, handle).await;
        }
        Err(error) => return Err(error),
    };
    let (container_id, lease_id) = {
        let state = execution
            .lock()
            .map_err(|_| internal_error_with_context("reading execution state for release"))?;
        if !state.terminal {
            return Err(ExecutionError::new(
                ExecutionErrorKind::PolicyDenied,
                "Running executions cannot be released",
                false,
            ));
        }
        (state.container_id.clone(), state.lease_id.clone())
    };
    let cleanup_started = std::time::Instant::now();
    verify_owned(backend, &container_id, &handle.execution_id).await?;
    tokio::time::timeout(
        Duration::from_secs(5),
        backend.docker.remove_container(
            &container_id,
            Some(RemoveContainerOptions {
                // Terminal execution can leave the long-lived workspace
                // keeper process running after its exec process has exited.
                // Ownership was verified above, so force removal is bounded
                // and avoids leaking that container.
                force: true,
                ..Default::default()
            }),
        ),
    )
    .await
    .map_err(|_| operation_timeout())?
    .map_err(|error| operation_disconnected("remove the execution container", &error))?;
    {
        let mut state = execution
            .lock()
            .map_err(|_| internal_error_with_context("recording execution cleanup completion"))?;
        state.cleanup_complete = true;
        state.phase_timings.cleanup_ms = Some(elapsed_millis(cleanup_started));
        state.persist()?;
    }
    let prepared = backend
        .workspaces
        .lock()
        .map_err(|_| internal_error_with_context("reading the workspace registry during release"))?
        .get(&lease_id)
        .cloned()
        .ok_or_else(|| internal_error_with_context("finding the workspace lease during release"))?;
    {
        let mut state = prepared.lock().map_err(|_| {
            internal_error_with_context("updating workspace execution state during release")
        })?;
        if state.active_execution_id.as_deref() != Some(handle.execution_id.as_str()) {
            return Err(internal_error_with_context(
                "checking workspace execution ownership during release",
            ));
        }
        state.active_execution_id = None;
    }
    backend
        .executions
        .lock()
        .map_err(|_| internal_error_with_context("removing the execution registry entry"))?
        .remove(&handle.execution_id);
    Ok(())
}

#[cfg(test)]
#[path = "control/tests.rs"]
mod tests;

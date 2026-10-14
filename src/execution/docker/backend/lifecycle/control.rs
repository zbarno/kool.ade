use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};
use std::time::Duration;

use bollard::query_parameters::{RemoveContainerOptions, StopContainerOptions};

use crate::execution::{
    backend::ExecutionHandleV1,
    contract::{ExecutionLifecycleV1, ExecutionResultV1},
    docker::backend::{DockerExecutionBackend, internal_error, invalid_request},
    errors::{ExecutionError, ExecutionErrorKind},
};

use super::runtime::RuntimeExecution;

mod cancel;
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
        .map_err(|_| internal_error())?
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

pub(super) async fn cancel(
    backend: &DockerExecutionBackend,
    handle: &ExecutionHandleV1,
    generation: u64,
) -> Result<(), ExecutionError> {
    let execution = match backend.lookup_execution(handle) {
        Ok(execution) => execution,
        Err(error) if error.kind == ExecutionErrorKind::WorkerDisconnected => {
            return cancel_persisted(backend, handle, generation).await;
        }
        Err(error) => return Err(error),
    };
    let (
        container_id,
        workspace,
        snapshot,
        budget,
        writable,
        authorization_generation,
        cancellation_intent,
    ) = {
        let state = execution.lock().map_err(|_| internal_error())?;
        if state.terminal {
            return Ok(());
        }
        if state.request.cancellation_generation != generation {
            return Err(invalid_request());
        }
        let cancellation_intent =
            CancellationIntentGuard::new(Arc::clone(&state.cancellation_requests));
        (
            state.container_id.clone(),
            state._workspace.clone(),
            state.workspace_snapshot.clone(),
            state.request.resource_budget.temporary_disk_bytes,
            super::execute::workspace_write_enabled(&state.request),
            state.request.authorization_generation.clone(),
            cancellation_intent,
        )
    };
    let _cancellation_intent = cancellation_intent;
    if let Err(error) = verify_owned(backend, &container_id, &handle.execution_id).await {
        terminal(
            &execution,
            ExecutionLifecycleV1::Interrupted,
            Some(error.clone()),
            None,
            false,
            false,
        )?;
        return Err(error);
    }
    let stopped = tokio::time::timeout(
        Duration::from_secs(6),
        backend.docker.stop_container(
            &container_id,
            Some(StopContainerOptions {
                t: Some(5),
                ..Default::default()
            }),
        ),
    )
    .await
    .is_ok_and(|result| result.is_ok());
    if !stopped {
        let killed = tokio::time::timeout(
            Duration::from_secs(2),
            backend.docker.kill_container(
                &container_id,
                None::<bollard::query_parameters::KillContainerOptions>,
            ),
        )
        .await;
        let result = match killed {
            Ok(Ok(())) => Ok(()),
            Ok(Err(_)) => Err(disconnected()),
            Err(_) => Err(operation_timeout()),
        };
        if let Err(error) = result {
            terminal(
                &execution,
                ExecutionLifecycleV1::Interrupted,
                Some(error.clone()),
                None,
                false,
                false,
            )?;
            return Err(error);
        }
    }
    if let Err(error) = super::execute::sync_workspace(
        backend,
        &container_id,
        &workspace,
        snapshot.as_ref(),
        budget,
        writable,
        &authorization_generation,
    )
    .await
    {
        terminal(
            &execution,
            ExecutionLifecycleV1::Interrupted,
            Some(error.clone()),
            None,
            false,
            false,
        )?;
        return Err(error);
    }
    terminal(
        &execution,
        ExecutionLifecycleV1::Cancelled,
        Some(ExecutionError::new(
            ExecutionErrorKind::Cancelled,
            "Execution was cancelled",
            false,
        )),
        None,
        false,
        true,
    )?;
    Ok(())
}

pub(super) async fn collect(
    backend: &DockerExecutionBackend,
    handle: &ExecutionHandleV1,
) -> Result<ExecutionResultV1, ExecutionError> {
    let execution = match backend.lookup_execution(handle) {
        Ok(execution) => execution,
        Err(error) if error.kind == ExecutionErrorKind::WorkerDisconnected => {
            let record = load_record(backend, handle)?;
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
        .map_err(|_| internal_error())?
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
        let state = execution.lock().map_err(|_| internal_error())?;
        if !state.terminal {
            return Err(ExecutionError::new(
                ExecutionErrorKind::PolicyDenied,
                "Running executions cannot be released",
                false,
            ));
        }
        (state.container_id.clone(), state.lease_id.clone())
    };
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
        let mut state = execution.lock().map_err(|_| internal_error())?;
        state.cleanup_complete = true;
        state.persist()?;
    }
    let prepared = backend
        .workspaces
        .lock()
        .map_err(|_| internal_error())?
        .get(&lease_id)
        .cloned()
        .ok_or_else(internal_error)?;
    {
        let mut state = prepared.lock().map_err(|_| internal_error())?;
        if state.active_execution_id.as_deref() != Some(handle.execution_id.as_str()) {
            return Err(internal_error());
        }
        state.active_execution_id = None;
    }
    backend
        .executions
        .lock()
        .map_err(|_| internal_error())?
        .remove(&handle.execution_id);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::CancellationIntentGuard;
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };

    #[tokio::test]
    async fn aborted_cancellation_releases_its_terminal_result_reservation() {
        let requests = Arc::new(AtomicUsize::new(0));
        let task_requests = Arc::clone(&requests);
        let task = tokio::spawn(async move {
            let _intent = CancellationIntentGuard::new(task_requests);
            std::future::pending::<()>().await;
        });
        tokio::task::yield_now().await;
        assert_eq!(requests.load(Ordering::Acquire), 1);

        task.abort();
        let _ = task.await;

        assert_eq!(requests.load(Ordering::Acquire), 0);
    }
}

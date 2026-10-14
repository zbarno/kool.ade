use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};
use std::time::Duration;

use bollard::query_parameters::RemoveContainerOptions;

use crate::execution::{
    backend::ExecutionHandleV1,
    contract::{ExecutionLifecycleV1, ExecutionResultV1},
    docker::backend::{DockerExecutionBackend, internal_error, invalid_request},
    errors::{ExecutionError, ExecutionErrorKind},
};

use super::runtime::RuntimeExecution;

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

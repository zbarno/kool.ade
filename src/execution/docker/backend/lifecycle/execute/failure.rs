use std::sync::{Arc, Mutex, atomic::Ordering};
use std::time::Instant;

use super::super::runtime::RuntimeExecution;
use crate::execution::{
    contract::ExecutionLifecycleV1,
    docker::backend::DockerExecutionBackend,
    errors::{ExecutionError, ExecutionErrorKind},
};

/// Capture, stop, and publish one terminal result under the same lock.
/// A cancellation arriving during cleanup inherits the preserved snapshot.
pub(super) async fn finalize_failure(
    backend: &DockerExecutionBackend,
    execution: &Arc<Mutex<RuntimeExecution>>,
    failure: ExecutionError,
    timed_out: bool,
) -> Result<(), ExecutionError> {
    let (workspace_sync, container_id, workspace, snapshot, request) = {
        let state = execution.lock().map_err(|_| super::state::disconnected())?;
        (
            Arc::clone(&state.workspace_sync),
            state.container_id.clone(),
            state._workspace.clone(),
            state.workspace_snapshot.clone(),
            state.request.clone(),
        )
    };
    let _workspace_sync = workspace_sync.lock().await;
    {
        let state = execution.lock().map_err(|_| super::state::disconnected())?;
        if state.terminal || state.cancellation_requests.load(Ordering::Acquire) > 0 {
            return Ok(());
        }
    }
    let stage_out_expected = super::sync::workspace_stage_out_enabled(&request);
    let stage_out_started = Instant::now();
    let cleanup = super::termination::terminate_and_sync(
        backend,
        &container_id,
        &workspace,
        snapshot.as_ref(),
        &request,
    )
    .await;
    if stage_out_expected {
        super::state::persist_stage_out(
            execution,
            stage_out_started
                .elapsed()
                .as_millis()
                .min(u128::from(u64::MAX)) as u64,
            cleanup.as_ref().ok().map(Vec::as_slice),
        )?;
    }
    let cancelled = execution
        .lock()
        .map_err(|_| super::state::disconnected())?
        .cancellation_requests
        .load(Ordering::Acquire)
        > 0;
    let (state, error, timed_out, cancelled) = match &cleanup {
        Err(error) => (
            ExecutionLifecycleV1::Interrupted,
            error.clone(),
            false,
            false,
        ),
        Ok(_) if cancelled => (
            ExecutionLifecycleV1::Cancelled,
            ExecutionError::new(
                ExecutionErrorKind::Cancelled,
                "Execution was cancelled",
                false,
            ),
            false,
            true,
        ),
        Ok(_) => (ExecutionLifecycleV1::Failed, failure, timed_out, false),
    };
    // This transaction owns workspace finalization. Publish even if another
    // cancellation is waiting; its lock recheck will see the terminal result.
    super::super::control::terminal(execution, state, Some(error), None, timed_out, cancelled)?;
    cleanup.map(|_| ())
}

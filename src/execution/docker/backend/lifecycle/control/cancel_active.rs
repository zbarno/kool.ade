use super::{
    CancellationIntentGuard, DockerExecutionBackend, ExecutionError, ExecutionErrorKind,
    ExecutionHandleV1, ExecutionLifecycleV1, cancel_persisted, internal_error_with_context,
    invalid_request, terminal,
};
use std::{sync::Arc, time::Instant};

pub(in crate::execution::docker::backend::lifecycle) async fn cancel_active(
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
    let (container_id, workspace, snapshot, request, cancellation_intent, workspace_sync) = {
        let state = execution.lock().map_err(|_| {
            internal_error_with_context("reading execution state for active cancellation")
        })?;
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
            state.request.clone(),
            cancellation_intent,
            Arc::clone(&state.workspace_sync),
        )
    };
    let _cancellation_intent = cancellation_intent;
    let _workspace_sync = workspace_sync.lock().await;
    if execution
        .lock()
        .map_err(|_| {
            internal_error_with_context("checking execution state before active cancellation")
        })?
        .terminal
    {
        return Ok(());
    }
    let stage_out_expected = super::super::execute::workspace_stage_out_enabled(&request);
    let stage_out_started = Instant::now();
    let synchronized = super::super::execute::terminate_and_sync(
        backend,
        &container_id,
        &workspace,
        snapshot.as_ref(),
        &request,
    )
    .await;
    let stage_out_ms = super::elapsed_millis(stage_out_started);
    let artifacts = match synchronized {
        Ok(artifacts) => artifacts,
        Err(error) => {
            if stage_out_expected {
                super::super::execute::persist_stage_out(&execution, stage_out_ms, None)?;
            }
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
    };
    if stage_out_expected {
        super::super::execute::persist_stage_out(&execution, stage_out_ms, Some(&artifacts))?;
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

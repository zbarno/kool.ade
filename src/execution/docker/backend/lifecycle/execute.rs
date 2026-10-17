use bollard::exec::StartExecOptions;
use std::{sync::Arc, time::Instant};
use tokio::io::AsyncWriteExt;

mod cleanup;
mod failure;
mod finish;
mod initial;
mod resource_metrics;
mod state;
mod stream;
mod termination;
#[cfg(test)]
pub(in crate::execution::docker::backend) use termination::observe_cancel_race_archive;
pub(super) use termination::terminate_and_sync;
mod sync;
#[cfg(test)]
pub(in crate::execution::docker::backend) use finish::{
    inject_crash_after_stage_out, pause_after_stage_out, pause_before_collecting,
};
pub(in crate::execution::docker::backend::lifecycle) use state::persist_stage_out;
pub(in crate::execution::docker::backend::lifecycle) use sync::workspace_stage_out_enabled;
pub(in crate::execution::docker::backend::lifecycle) use sync::workspace_write_enabled;

use crate::execution::{
    backend::ExecutionHandleV1,
    contract::ExecutionRequestV1,
    docker::backend::{DockerExecutionBackend, internal_error_with_context},
    errors::{ExecutionError, ExecutionErrorKind},
};

pub(super) async fn run(
    backend: &DockerExecutionBackend,
    handle: &ExecutionHandleV1,
    request: &ExecutionRequestV1,
) -> Result<(), ExecutionError> {
    let (execution, container_id, budget, deadline, saved_attempt, workspace, snapshot) =
        initial::begin(backend, handle, request)?;
    if let Err(error) = saved_attempt {
        let _ = failure::finalize_failure(backend, &execution, error.clone(), false).await;
        return Err(error);
    }
    let rpc_deadline = tokio::time::Instant::now() + cleanup::remaining(deadline);
    let ownership = tokio::time::timeout_at(
        rpc_deadline,
        super::control::verify_owned(backend, &container_id, &handle.execution_id),
    )
    .await;
    if !matches!(ownership, Ok(Ok(()))) {
        if cleanup::remaining(deadline).is_zero() {
            return cleanup::deadline_failure(backend, &execution).await;
        }
        return state::interrupted(&execution);
    }
    let config = initial::command_config(request);
    let exec = match tokio::time::timeout_at(
        rpc_deadline,
        backend.docker.create_exec(&container_id, config),
    )
    .await
    {
        Ok(Ok(exec)) => exec,
        Ok(Err(error)) => {
            return state::interrupted_with(
                &execution,
                state::operation_disconnected("create the workspace command", &error),
            );
        }
        Err(_) => {
            return cleanup::deadline_failure(backend, &execution).await;
        }
    };
    #[cfg(test)]
    state::mark_diagnostic_progress(&execution, state::EXEC_CREATED);
    {
        let mut state = execution
            .lock()
            .map_err(|_| internal_error_with_context("recording the container command ID"))?;
        state.command_id = Some(exec.id.clone());
        state.persist()?;
    }
    let stream = match tokio::time::timeout_at(
        rpc_deadline,
        backend.docker.start_exec(
            &exec.id,
            Some(StartExecOptions {
                detach: false,
                tty: false,
                output_capacity: Some(64 * 1024),
            }),
        ),
    )
    .await
    {
        Ok(Ok(stream)) => stream,
        Ok(Err(error)) => {
            return state::interrupted_with(
                &execution,
                state::operation_disconnected("attach to the workspace command", &error),
            );
        }
        Err(_) => {
            return cleanup::deadline_failure(backend, &execution).await;
        }
    };
    #[cfg(test)]
    state::mark_diagnostic_progress(&execution, state::EXEC_ATTACHED);
    let stream_budget = cleanup::remaining(deadline);
    let (metrics_stop, metrics_stop_rx) = tokio::sync::oneshot::channel();
    let metrics_execution = Arc::downgrade(&execution);
    let metrics_docker = backend.docker.clone();
    let metrics_container_id = container_id.clone();
    let metrics_task = tokio::spawn(async move {
        resource_metrics::collect(
            metrics_docker,
            metrics_container_id,
            metrics_execution,
            metrics_stop_rx,
        )
        .await;
    });
    let collect = async {
        match stream {
            bollard::exec::StartExecResults::Attached { output, mut input } => {
                let script = request
                    .stdin
                    .as_deref()
                    .unwrap_or_default()
                    .as_bytes()
                    .to_vec();
                #[cfg(test)]
                let input_execution = execution.clone();
                let write_input = async move {
                    if !script.is_empty() {
                        input.write_all(&script).await.map_err(|_| {
                            ExecutionError::new(
                                ExecutionErrorKind::ExecutionFailed,
                                "Container command input could not be sent",
                                false,
                            )
                        })?;
                    }
                    #[cfg(test)]
                    state::mark_diagnostic_progress(&input_execution, state::INPUT_WRITTEN);
                    input.shutdown().await.map_err(|_| {
                        ExecutionError::new(
                            ExecutionErrorKind::ExecutionFailed,
                            "Container command input could not be closed",
                            false,
                        )
                    })?;
                    #[cfg(test)]
                    state::mark_diagnostic_progress(&input_execution, state::INPUT_CLOSED);
                    Ok(())
                };
                futures_util::try_join!(
                    write_input,
                    stream::collect_output(output, execution.clone(), budget)
                )?;
                Ok(())
            }
            bollard::exec::StartExecResults::Detached => Err(ExecutionError::new(
                ExecutionErrorKind::WorkerDisconnected,
                "Docker did not attach the bounded command stream",
                true,
            )),
        }
    };
    let command_started = Instant::now();
    let collected = tokio::time::timeout(stream_budget, collect).await;
    let _ = metrics_stop.send(());
    let _ = metrics_task.await;
    if request.mode == crate::execution::contract::ExecutionModeV1::RuntimeQa {
        let mut state = execution
            .lock()
            .map_err(|_| internal_error_with_context("recording runtime QA execution telemetry"))?;
        state.phase_timings.qa_ms = Some(elapsed_millis(command_started));
        state.persist()?;
    }
    match collected {
        Ok(Ok(())) => {}
        Ok(Err(error)) => {
            failure::finalize_failure(backend, &execution, error, false).await?;
            return Ok(());
        }
        Err(_) => {
            cleanup::deadline_failure(backend, &execution).await?;
            return Ok(());
        }
    }
    finish::finish_command(
        backend,
        &execution,
        request,
        &container_id,
        &exec.id,
        &workspace,
        snapshot.as_ref(),
    )
    .await
}

fn elapsed_millis(started: Instant) -> u64 {
    started.elapsed().as_millis().min(u128::from(u64::MAX)) as u64
}

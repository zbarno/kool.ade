use bollard::exec::StartExecOptions;
use tokio::io::AsyncWriteExt;

mod cleanup;
mod failure;
mod finish;
mod initial;
mod state;
mod stream;
mod termination;
pub(super) use termination::terminate_and_sync;
mod sync;
pub(in crate::execution::docker::backend::lifecycle) use sync::workspace_write_enabled;

use crate::execution::{
    backend::ExecutionHandleV1,
    contract::ExecutionRequestV1,
    docker::backend::{DockerExecutionBackend, internal_error},
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
    {
        let mut state = execution.lock().map_err(|_| internal_error())?;
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
    let stream_budget = cleanup::remaining(deadline);
    let collect = async {
        match stream {
            bollard::exec::StartExecResults::Attached { output, mut input } => {
                let script = request
                    .stdin
                    .as_deref()
                    .unwrap_or_default()
                    .as_bytes()
                    .to_vec();
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
                    input.shutdown().await.map_err(|_| {
                        ExecutionError::new(
                            ExecutionErrorKind::ExecutionFailed,
                            "Container command input could not be closed",
                            false,
                        )
                    })
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
    match tokio::time::timeout(stream_budget, collect).await {
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

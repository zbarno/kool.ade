use bollard::exec::StartExecOptions;
use tokio::io::AsyncWriteExt;

mod cleanup;
mod initial;
mod state;
mod stream;
mod termination;
pub(super) use termination::terminate_and_sync;
mod sync;
pub(in crate::execution::docker::backend::lifecycle) use sync::workspace_write_enabled;

use crate::execution::{
    backend::ExecutionHandleV1,
    contract::{ExecutionLifecycleV1, ExecutionRequestV1},
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
        let _ = cleanup::stop_owned(backend, &container_id, &handle.execution_id).await;
        let _ = state::interrupted(&execution);
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
            return cleanup::deadline_failure(
                backend,
                &execution,
                &container_id,
                &handle.execution_id,
            )
            .await;
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
            return cleanup::deadline_failure(
                backend,
                &execution,
                &container_id,
                &handle.execution_id,
            )
            .await;
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
            return cleanup::deadline_failure(
                backend,
                &execution,
                &container_id,
                &handle.execution_id,
            )
            .await;
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
            let cleanup = sync::terminate_if_active(
                &execution,
                backend,
                &container_id,
                &workspace,
                snapshot.as_ref(),
                request,
            )
            .await;
            if matches!(cleanup, Ok(false)) {
                return Ok(());
            }
            let failure = cleanup.as_ref().err().cloned().unwrap_or(error);
            state::finish(
                &execution,
                if cleanup.is_ok() {
                    ExecutionLifecycleV1::Failed
                } else {
                    ExecutionLifecycleV1::Interrupted
                },
                Some(failure.clone()),
                None,
                false,
                false,
            )?;
            return Err(failure);
        }
        Err(_) => {
            let cleanup = sync::terminate_if_active(
                &execution,
                backend,
                &container_id,
                &workspace,
                snapshot.as_ref(),
                request,
            )
            .await;
            if matches!(cleanup, Ok(false)) {
                return Ok(());
            }
            let failure = cleanup.as_ref().err().cloned().unwrap_or_else(|| {
                ExecutionError::new(
                    ExecutionErrorKind::DeadlineExceeded,
                    "Execution exceeded its deadline",
                    false,
                )
            });
            state::finish(
                &execution,
                if cleanup.is_ok() {
                    ExecutionLifecycleV1::Failed
                } else {
                    ExecutionLifecycleV1::Interrupted
                },
                Some(failure),
                None,
                true,
                false,
            )?;
            return Ok(());
        }
    }
    let missing_exit_status_deadline =
        tokio::time::Instant::now() + std::time::Duration::from_secs(5);
    let inspected = loop {
        let inspected = match tokio::time::timeout_at(
            tokio::time::Instant::now() + cleanup::remaining(deadline),
            backend.docker.inspect_exec(&exec.id),
        )
        .await
        {
            Ok(Ok(inspected)) => inspected,
            Ok(Err(error)) => {
                return state::interrupted_with(
                    &execution,
                    state::operation_disconnected("inspect the workspace command", &error),
                );
            }
            Err(_) => {
                return cleanup::deadline_failure(
                    backend,
                    &execution,
                    &container_id,
                    &handle.execution_id,
                )
                .await;
            }
        };
        if inspected.exit_code.is_some() {
            break inspected;
        }
        if inspected.running == Some(false)
            && tokio::time::Instant::now() >= missing_exit_status_deadline
        {
            state::interrupted_with(
                &execution,
                ExecutionError::new(
                    ExecutionErrorKind::WorkerDisconnected,
                    "Docker stopped the command without recording its exit status",
                    true,
                ),
            )?;
            return Ok(());
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    };
    let code = inspected
        .exit_code
        .and_then(|code| i32::try_from(code).ok());
    let Some(code) = code else {
        state::interrupted_with(
            &execution,
            ExecutionError::new(
                ExecutionErrorKind::WorkerDisconnected,
                "Docker returned an invalid command exit status",
                true,
            ),
        )?;
        return Ok(());
    };
    let synced = match sync::sync_workspace_if_active(
        &execution,
        sync::WorkspaceSyncRequest {
            backend,
            container_id: &container_id,
            workspace: &workspace,
            snapshot: snapshot.as_ref(),
            budget: request.resource_budget.temporary_disk_bytes,
            writable: sync::workspace_write_enabled(request),
            authorization_generation: &request.authorization_generation,
        },
    )
    .await
    {
        Ok(synced) => synced,
        Err(error) => {
            state::finish(
                &execution,
                ExecutionLifecycleV1::Failed,
                Some(error.clone()),
                Some(code),
                false,
                false,
            )?;
            return Err(error);
        }
    };
    if !synced {
        return Ok(());
    }
    let failure = (code != 0).then(|| {
        ExecutionError::new(
            ExecutionErrorKind::ExecutionFailed,
            "Command exited unsuccessfully",
            false,
        )
    });
    state::finish(
        &execution,
        if failure.is_some() {
            ExecutionLifecycleV1::Failed
        } else {
            ExecutionLifecycleV1::Succeeded
        },
        failure,
        Some(code),
        false,
        false,
    )?;
    Ok(())
}

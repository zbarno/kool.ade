use std::time::Duration;

use bollard::exec::{CreateExecOptions, StartExecOptions, StartExecResults};
use futures_util::StreamExt;

use crate::execution::{
    contract::ExecutionRequestV1,
    docker::{
        backend::DockerExecutionBackend,
        sandbox::{ValidatedWorkspace, WorkspaceSnapshot, staging},
    },
    errors::ExecutionError,
};

mod guard;

/// Terminate task processes while retaining the container's workspace tmpfs.
/// Only after the bounded live snapshot is captured may the container stop.
pub(in crate::execution::docker::backend::lifecycle) async fn terminate_and_sync(
    backend: &DockerExecutionBackend,
    container_id: &str,
    workspace: &ValidatedWorkspace,
    snapshot: Option<&WorkspaceSnapshot>,
    request: &ExecutionRequestV1,
) -> Result<(), ExecutionError> {
    super::super::control::verify_owned(backend, container_id, &request.execution_id).await?;
    let mut guard = guard::StopOnDrop::new(backend, container_id, &request.execution_id);
    let captured = if super::sync::workspace_write_enabled(request) {
        tokio::time::timeout(Duration::from_secs(60), async {
            backend.verify_authorization(&request.authorization_generation)?;
            let baseline = snapshot.ok_or_else(super::state::disconnected)?;
            quiesce(backend, container_id).await?;
            let archive = super::sync::download_workspace(
                backend,
                container_id,
                request.resource_budget.temporary_disk_bytes,
            )
            .await?;
            Ok::<_, ExecutionError>(Some((baseline, archive)))
        })
        .await
        .map_err(|_| super::super::control::operation_timeout())
        .and_then(|result| result)
    } else {
        Ok(None)
    };
    // Always terminate, including when capture or authorization failed.
    let stopped = super::cleanup::stop_owned(backend, container_id, &request.execution_id).await;
    if stopped.is_ok() {
        guard.disarm();
    }
    stopped?;
    if let Some((baseline, archive)) = captured? {
        backend.verify_authorization(&request.authorization_generation)?;
        staging::synchronize(
            workspace,
            baseline,
            &archive,
            request.resource_budget.temporary_disk_bytes,
        )?;
    }
    Ok(())
}

async fn quiesce(
    backend: &DockerExecutionBackend,
    container_id: &str,
) -> Result<(), ExecutionError> {
    // Linux kill(-1) excludes PID 1 and the caller. Inside this container's
    // private PID namespace, the nonroot executor can only kill its own task
    // processes. The app-owned PID 1 keeper retains tmpfs until capture ends.
    let exec = backend
        .docker
        .create_exec(
            container_id,
            CreateExecOptions::<String> {
                attach_stdin: Some(false),
                attach_stdout: Some(true),
                attach_stderr: Some(true),
                cmd: Some(vec!["/bin/sh".into(), "-c".into(), "kill -KILL -1".into()]),
                user: Some("10001:10001".into()),
                privileged: Some(false),
                ..Default::default()
            },
        )
        .await
        .map_err(|error| super::state::operation_disconnected("stop task processes", &error))?;
    let started = backend
        .docker
        .start_exec(
            &exec.id,
            Some(StartExecOptions {
                detach: false,
                tty: false,
                ..Default::default()
            }),
        )
        .await
        .map_err(|error| super::state::operation_disconnected("stop task processes", &error))?;
    let StartExecResults::Attached { mut output, .. } = started else {
        return Err(super::state::disconnected());
    };
    // Wait for shutdown itself to finish before creating the archive exec.
    // Inspection alone can observe an exec that has not started yet.
    while let Some(chunk) = output.next().await {
        chunk.map_err(|error| {
            super::state::operation_disconnected("wait for task shutdown", &error)
        })?;
    }
    loop {
        let status = backend
            .docker
            .inspect_exec(&exec.id)
            .await
            .map_err(|error| {
                super::state::operation_disconnected("inspect task shutdown", &error)
            })?;
        if status.running == Some(false) && status.exit_code.is_some() {
            return if status.exit_code == Some(0) {
                Ok(())
            } else {
                Err(super::state::disconnected())
            };
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
}

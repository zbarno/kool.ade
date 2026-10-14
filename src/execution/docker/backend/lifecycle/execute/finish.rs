use std::sync::{Arc, Mutex};

use super::super::runtime::RuntimeExecution;
use super::{cleanup, state, sync};
use crate::execution::{
    contract::{ExecutionLifecycleV1, ExecutionRequestV1},
    docker::{
        backend::DockerExecutionBackend,
        sandbox::{ValidatedWorkspace, WorkspaceSnapshot},
    },
    errors::{ExecutionError, ExecutionErrorKind},
};

pub(super) async fn finish_command(
    backend: &DockerExecutionBackend,
    execution: &Arc<Mutex<RuntimeExecution>>,
    request: &ExecutionRequestV1,
    container_id: &str,
    command_id: &str,
    workspace: &ValidatedWorkspace,
    snapshot: Option<&WorkspaceSnapshot>,
) -> Result<(), ExecutionError> {
    let missing_exit_status_deadline =
        tokio::time::Instant::now() + std::time::Duration::from_secs(5);
    let inspected = loop {
        let inspected = match tokio::time::timeout_at(
            tokio::time::Instant::now() + cleanup::remaining(request.deadline_unix_ms),
            backend.docker.inspect_exec(command_id),
        )
        .await
        {
            Ok(Ok(inspected)) => inspected,
            Ok(Err(error)) => {
                return state::interrupted_with(
                    execution,
                    state::operation_disconnected("inspect the workspace command", &error),
                );
            }
            Err(_) => {
                return cleanup::deadline_failure(backend, execution).await;
            }
        };
        if inspected.exit_code.is_some() {
            break inspected;
        }
        if inspected.running == Some(false)
            && tokio::time::Instant::now() >= missing_exit_status_deadline
        {
            state::interrupted_with(
                execution,
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
            execution,
            ExecutionError::new(
                ExecutionErrorKind::WorkerDisconnected,
                "Docker returned an invalid command exit status",
                true,
            ),
        )?;
        return Ok(());
    };
    let synced = match sync::sync_workspace_if_active(
        execution,
        sync::WorkspaceSyncRequest {
            backend,
            container_id,
            workspace,
            snapshot,
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
                execution,
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
        execution,
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

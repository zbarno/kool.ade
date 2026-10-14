use std::sync::{Arc, Mutex};

use crate::execution::{
    docker::{
        backend::DockerExecutionBackend,
        sandbox::{ValidatedWorkspace, WorkspaceSnapshot},
    },
    errors::ExecutionError,
};

use super::super::super::runtime::RuntimeExecution;

pub(in crate::execution::docker::backend::lifecycle) struct WorkspaceSyncRequest<'a> {
    pub backend: &'a DockerExecutionBackend,
    pub container_id: &'a str,
    pub workspace: &'a ValidatedWorkspace,
    pub snapshot: Option<&'a WorkspaceSnapshot>,
    pub budget: u64,
    pub writable: bool,
    pub authorization_generation: &'a str,
}

pub(in crate::execution::docker::backend::lifecycle) async fn sync_workspace_if_active(
    execution: &Arc<Mutex<RuntimeExecution>>,
    request: WorkspaceSyncRequest<'_>,
) -> Result<bool, ExecutionError> {
    let workspace_sync = {
        let state = execution
            .lock()
            .map_err(|_| super::super::state::disconnected())?;
        Arc::clone(&state.workspace_sync)
    };
    let _workspace_sync = workspace_sync.lock().await;
    let active = {
        let state = execution
            .lock()
            .map_err(|_| super::super::state::disconnected())?;
        !state.terminal
            && state
                .cancellation_requests
                .load(std::sync::atomic::Ordering::Acquire)
                == 0
    };
    if !active {
        return Ok(false);
    }
    super::sync_workspace(
        request.backend,
        request.container_id,
        request.workspace,
        request.snapshot,
        request.budget,
        request.writable,
        request.authorization_generation,
    )
    .await?;
    Ok(true)
}

pub(in crate::execution::docker::backend::lifecycle) async fn terminate_if_active(
    execution: &Arc<Mutex<RuntimeExecution>>,
    backend: &DockerExecutionBackend,
    container_id: &str,
    workspace: &ValidatedWorkspace,
    snapshot: Option<&WorkspaceSnapshot>,
    request: &crate::execution::contract::ExecutionRequestV1,
) -> Result<bool, ExecutionError> {
    let workspace_sync = Arc::clone(
        &execution
            .lock()
            .map_err(|_| super::super::state::disconnected())?
            .workspace_sync,
    );
    let _workspace_sync = workspace_sync.lock().await;
    {
        let state = execution
            .lock()
            .map_err(|_| super::super::state::disconnected())?;
        if state.terminal
            || state
                .cancellation_requests
                .load(std::sync::atomic::Ordering::Acquire)
                > 0
        {
            return Ok(false);
        }
    }
    super::super::termination::terminate_and_sync(
        backend,
        container_id,
        workspace,
        snapshot,
        request,
    )
    .await?;
    Ok(true)
}

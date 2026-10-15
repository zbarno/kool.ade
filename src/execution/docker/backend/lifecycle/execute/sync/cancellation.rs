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
    pub artifact_paths: &'a [String],
    pub authorization_generation: &'a str,
}

pub(in crate::execution::docker::backend::lifecycle) async fn sync_workspace_if_active(
    execution: &Arc<Mutex<RuntimeExecution>>,
    request: WorkspaceSyncRequest<'_>,
) -> Result<Option<Vec<crate::execution::contract::ArtifactManifestEntryV1>>, ExecutionError> {
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
        return Ok(None);
    }
    let artifacts = super::sync_workspace(request).await?;
    Ok(Some(artifacts))
}

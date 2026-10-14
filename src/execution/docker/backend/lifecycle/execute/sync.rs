use bollard::query_parameters::DownloadFromContainerOptionsBuilder;
use futures_util::StreamExt;

use crate::execution::{
    contract::ExecutionRequestV1,
    docker::{
        backend::DockerExecutionBackend,
        sandbox::{ValidatedWorkspace, WorkspaceSnapshot, staging},
    },
    errors::{ExecutionError, ExecutionErrorKind},
};

pub(in crate::execution::docker::backend::lifecycle) async fn sync_workspace(
    backend: &DockerExecutionBackend,
    container_id: &str,
    workspace: &ValidatedWorkspace,
    snapshot: Option<&WorkspaceSnapshot>,
    budget: u64,
    writable: bool,
    authorization_generation: &str,
) -> Result<(), ExecutionError> {
    if !writable {
        return Ok(());
    }
    backend.verify_authorization(authorization_generation)?;
    let Some(snapshot) = snapshot else {
        return Err(ExecutionError::new(
            ExecutionErrorKind::WorkerDisconnected,
            "Workspace baseline is unavailable after recovery; task clone was preserved",
            false,
        ));
    };
    let archive = download_workspace(backend, container_id, budget).await?;
    backend.verify_authorization(authorization_generation)?;
    staging::synchronize(workspace, snapshot, &archive, budget)
}

pub(in crate::execution::docker::backend::lifecycle) fn workspace_write_enabled(
    request: &ExecutionRequestV1,
) -> bool {
    request.mode != crate::execution::contract::ExecutionModeV1::PlanningReadOnly
        && request.policy.filesystem_grants().iter().any(|grant| {
            matches!(
                grant,
                crate::execution::policy::FilesystemGrantV1::WorkspaceWrite
                    | crate::execution::policy::FilesystemGrantV1::WorkspaceScopedWrite
            )
        })
}

async fn download_workspace(
    backend: &DockerExecutionBackend,
    container_id: &str,
    budget: u64,
) -> Result<Vec<u8>, ExecutionError> {
    let limit = budget.min(crate::execution::docker::sandbox::staging::MAX_STAGED_BYTES);
    let options = DownloadFromContainerOptionsBuilder::default()
        .path("/workspace")
        .build();
    let mut stream = backend
        .docker
        .download_from_container(container_id, Some(options));
    let mut archive = Vec::new();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|error| {
            super::state::operation_disconnected("download the workspace archive", &error)
        })?;
        if (archive.len() as u64).saturating_add(chunk.len() as u64) > limit {
            return Err(ExecutionError::new(
                ExecutionErrorKind::ExecutionFailed,
                "Container workspace archive exceeded its synchronization limit",
                false,
            ));
        }
        archive.extend_from_slice(&chunk);
    }
    Ok(archive)
}

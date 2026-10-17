use std::time::Duration;

use crate::execution::{
    contract::ExecutionRequestV1,
    docker::{
        backend::DockerExecutionBackend,
        sandbox::{ValidatedWorkspace, WorkspaceSnapshot, staging},
    },
    errors::ExecutionError,
};

mod guard;
mod quiesce;

/// Terminate task processes while retaining the container's workspace tmpfs.
/// Only after the bounded live snapshot is captured may the container stop.
pub(in crate::execution::docker::backend::lifecycle) async fn terminate_and_sync(
    backend: &DockerExecutionBackend,
    container_id: &str,
    workspace: &ValidatedWorkspace,
    snapshot: Option<&WorkspaceSnapshot>,
    request: &ExecutionRequestV1,
) -> Result<Vec<crate::execution::contract::ArtifactManifestEntryV1>, ExecutionError> {
    super::super::control::verify_owned(backend, container_id, &request.execution_id).await?;
    let mut guard = guard::StopOnDrop::new(backend, container_id, &request.execution_id);
    let writable = super::sync::workspace_write_enabled(request);
    let artifact_paths = request.policy.artifact_paths();
    let captured = if writable || !artifact_paths.is_empty() {
        tokio::time::timeout(Duration::from_secs(60), async {
            backend.verify_authorization(&request.authorization_generation)?;
            let baseline = if writable {
                Some(snapshot.ok_or_else(super::state::disconnected)?)
            } else {
                None
            };
            quiesce::stop_tasks(backend, container_id, &request.execution_id).await?;
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
        return if let Some(baseline) = baseline {
            staging::synchronize_with_artifacts(
                workspace,
                baseline,
                &archive,
                request.resource_budget.temporary_disk_bytes,
                artifact_paths,
            )
        } else {
            staging::collect_artifacts(
                &archive,
                request.resource_budget.temporary_disk_bytes,
                artifact_paths,
            )
        };
    }
    Ok(Vec::new())
}

use std::sync::Arc;

use crate::execution::{
    backend::WorkspaceLeaseV1,
    contract::{ExecutionTarget, WorkspaceRef},
    docker::{
        backend::{DockerExecutionBackend, internal_error, invalid_request, now_unix_ms},
        sandbox::ValidatedWorkspace,
    },
    errors::ExecutionError,
};

pub(in crate::execution::docker::backend::lifecycle) async fn prepare(
    backend: &DockerExecutionBackend,
    workspace: &WorkspaceRef,
    target: &ExecutionTarget,
) -> Result<WorkspaceLeaseV1, ExecutionError> {
    if target != &ExecutionTarget::local() || workspace.validate().is_err() {
        return Err(invalid_request());
    }
    let path = backend.resolver.resolve_owned_clone(workspace)?;
    let verified = ValidatedWorkspace::inspect(&path)?;
    super::ensure_ready(backend, &verified).await?;
    let lease_id = uuid::Uuid::new_v4().hyphenated().to_string();
    let expires_unix_ms = now_unix_ms().saturating_add(30 * 60 * 1000);
    backend
        .workspaces
        .lock()
        .map_err(|_| internal_error())?
        .insert(
            lease_id.clone(),
            Arc::new(std::sync::Mutex::new(
                super::super::super::PreparedWorkspace {
                    workspace: workspace.clone(),
                    expires_unix_ms,
                    verified,
                    starting: false,
                    active_execution_id: None,
                },
            )),
        );
    Ok(WorkspaceLeaseV1 {
        lease_id,
        workspace: workspace.clone(),
        expires_unix_ms,
    })
}

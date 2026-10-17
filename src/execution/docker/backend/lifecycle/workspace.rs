use crate::execution::{
    backend::{BackendFuture, WorkspaceLeaseV1},
    docker::{
        backend::{DockerExecutionBackend, prerequisite_error},
        sandbox::ValidatedWorkspace,
    },
    errors::{ExecutionError, ExecutionErrorKind},
};

mod archive;
mod cleanup;
mod prepare;
mod probe;
mod start;

pub(super) use archive::extract_archive;
pub(super) use prepare::prepare;
pub(super) use probe::probe;
pub(super) use start::start;
#[cfg(test)]
pub(super) use start::{persist_runtime_wait_error, record_start_registration_failure};

pub(super) fn release_workspace<'a>(
    backend: &'a DockerExecutionBackend,
    lease: &'a WorkspaceLeaseV1,
) -> BackendFuture<'a, ()> {
    Box::pin(async move {
        let mut workspaces = backend.workspaces.lock().map_err(|_| {
            crate::execution::docker::backend::internal_error_with_context(
                "releasing a prepared workspace",
            )
        })?;
        let prepared = workspaces.get(&lease.lease_id).cloned().ok_or_else(|| {
            ExecutionError::new(
                ExecutionErrorKind::WorkerDisconnected,
                "Workspace lease is no longer owned by this backend",
                false,
            )
        })?;
        let state = prepared.lock().map_err(|_| {
            crate::execution::docker::backend::internal_error_with_context(
                "checking workspace release state",
            )
        })?;
        if state.workspace != lease.workspace
            || state.expires_unix_ms != lease.expires_unix_ms
            || state.starting
            || state.active_execution_id.is_some()
        {
            return Err(ExecutionError::new(
                ExecutionErrorKind::PolicyDenied,
                "Workspace lease is active or does not match this reservation",
                false,
            ));
        }
        workspaces.remove(&lease.lease_id).ok_or_else(|| {
            ExecutionError::new(
                ExecutionErrorKind::WorkerDisconnected,
                "Workspace lease is no longer owned by this backend",
                false,
            )
        })?;
        Ok(())
    })
}

async fn ensure_ready(
    backend: &DockerExecutionBackend,
    workspace: &ValidatedWorkspace,
) -> Result<crate::execution::docker::discovery::EngineReadiness, ExecutionError> {
    let readiness = backend
        .engine
        .probe(workspace.root())
        .await
        .map_err(prerequisite_error)?;
    if !readiness.ready {
        return Err(ExecutionError::new(
            ExecutionErrorKind::EnvironmentUnavailable,
            "Docker Engine prerequisites are not satisfied",
            false,
        ));
    }
    Ok(readiness)
}

use crate::execution::{
    docker::{
        backend::{DockerExecutionBackend, prerequisite_error},
        sandbox::ValidatedWorkspace,
    },
    errors::{ExecutionError, ExecutionErrorKind},
};

mod cleanup;
mod prepare;
mod probe;
mod start;

pub(super) use prepare::prepare;
pub(super) use probe::probe;
pub(super) use start::start;

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

use std::sync::Arc;

use super::{
    super::super::runtime::{RuntimeExecution, RuntimeExecutionContext},
    reservation::WorkspaceStartReservation,
};
use crate::execution::{
    backend::{ExecutionHandleV1, WorkspaceLeaseV1},
    contract::{ExecutionEventKindV1, ExecutionLifecycleV1, ExecutionRequestV1},
    docker::{
        backend::DockerExecutionBackend,
        sandbox::{ValidatedWorkspace, staging::WorkspaceSnapshot},
    },
    errors::ExecutionError,
};

#[allow(clippy::too_many_arguments)]
pub(super) fn new_execution(
    backend: &DockerExecutionBackend,
    lease: &WorkspaceLeaseV1,
    request: &ExecutionRequestV1,
    workspace: ValidatedWorkspace,
    snapshot: &WorkspaceSnapshot,
    container_name: String,
    lease_expires_at_unix_ms: u64,
    task_lock: std::fs::File,
) -> Result<RuntimeExecution, ExecutionError> {
    let mut execution = RuntimeExecution::new(
        request.clone(),
        RuntimeExecutionContext {
            container_id: String::new(),
            container_name,
            workspace,
            lease_id: lease.lease_id.clone(),
            owner_instance_id: backend.identity.owner_instance_id.clone(),
            policy_version: backend.identity.policy_version.clone(),
            lease_expires_at_unix_ms,
            task_lock,
            workspace_snapshot: Some(WorkspaceSnapshot {
                archive: Vec::new(),
                manifest: snapshot.manifest.clone(),
                scope_root: snapshot.scope_root.clone(),
            }),
        },
    );
    execution.persist()?;
    execution.push(ExecutionEventKindV1::Lifecycle {
        state: ExecutionLifecycleV1::Queued,
    })?;
    execution.push(ExecutionEventKindV1::Lifecycle {
        state: ExecutionLifecycleV1::Preparing,
    })?;
    Ok(execution)
}

pub(in crate::execution::docker::backend::lifecycle) fn record_runtime_wait(
    execution: &mut RuntimeExecution,
) -> Result<(), ExecutionError> {
    execution.push(ExecutionEventKindV1::Lifecycle {
        state: ExecutionLifecycleV1::WaitingForRuntime,
    })
}

pub(in crate::execution::docker::backend::lifecycle) fn persist_runtime_wait_error(
    execution: &mut RuntimeExecution,
    error: ExecutionError,
) -> Result<ExecutionError, ExecutionError> {
    record_runtime_wait(execution)?;
    Ok(error)
}

pub(in crate::execution::docker::backend::lifecycle) fn record_start_registration_failure(
    execution: &mut RuntimeExecution,
    error: ExecutionError,
) -> ExecutionError {
    match execution.fail_start(error.clone()) {
        Ok(()) => error,
        Err(persistence_error) => persistence_error,
    }
}

pub(super) fn register_started_execution(
    backend: &DockerExecutionBackend,
    handle: &ExecutionHandleV1,
    mut execution: RuntimeExecution,
    reservation: &mut WorkspaceStartReservation,
) -> Result<(), Box<(RuntimeExecution, ExecutionError)>> {
    let mut executions = match backend.executions.lock() {
        Ok(executions) => executions,
        Err(poisoned) => {
            drop(poisoned);
            return Err(Box::new((
                execution,
                crate::execution::docker::backend::internal_error_with_context(
                    "updating the execution registry",
                ),
            )));
        }
    };
    if let Err(error) = reservation.activate(&handle.execution_id) {
        return Err(Box::new((execution, error)));
    }
    if let Err(error) = execution.push(ExecutionEventKindV1::Lifecycle {
        state: ExecutionLifecycleV1::Running,
    }) {
        return Err(Box::new((execution, error)));
    }
    executions.insert(
        handle.execution_id.clone(),
        Arc::new(std::sync::Mutex::new(execution)),
    );
    reservation.commit();
    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub(super) async fn ensure_runtime_ready(
    backend: &DockerExecutionBackend,
    lease: &WorkspaceLeaseV1,
    request: &ExecutionRequestV1,
    workspace: &ValidatedWorkspace,
    snapshot: &WorkspaceSnapshot,
    container_name: &str,
    lease_expires_at_unix_ms: u64,
    task_lock: std::fs::File,
    deadline: tokio::time::Instant,
) -> Result<
    (
        crate::execution::docker::discovery::EngineReadiness,
        std::fs::File,
    ),
    ExecutionError,
> {
    match tokio::time::timeout_at(deadline, super::super::ensure_ready(backend, workspace)).await {
        Ok(Ok(readiness)) => Ok((readiness, task_lock)),
        Ok(Err(error))
            if matches!(
                error.kind,
                crate::execution::errors::ExecutionErrorKind::EnvironmentUnavailable
                    | crate::execution::errors::ExecutionErrorKind::WorkerDisconnected
            ) =>
        {
            let mut execution = new_execution(
                backend,
                lease,
                request,
                workspace.clone(),
                snapshot,
                container_name.to_owned(),
                lease_expires_at_unix_ms,
                task_lock,
            )?;
            Err(persist_runtime_wait_error(&mut execution, error)?)
        }
        Ok(Err(error)) => Err(error),
        Err(_) => {
            let mut execution = new_execution(
                backend,
                lease,
                request,
                workspace.clone(),
                snapshot,
                container_name.to_owned(),
                lease_expires_at_unix_ms,
                task_lock,
            )?;
            Err(persist_runtime_wait_error(
                &mut execution,
                super::start_helpers::deadline_error(),
            )?)
        }
    }
}

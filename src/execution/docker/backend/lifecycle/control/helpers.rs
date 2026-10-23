use std::{
    sync::{Arc, Mutex},
    time::Duration,
};

use bollard::query_parameters::InspectContainerOptionsBuilder;

use crate::execution::{
    contract::{ExecutionEventKindV1, ExecutionLifecycleV1, ExecutionResultV1, ResourceMetricsV1},
    docker::backend::DockerExecutionBackend,
    errors::{ExecutionError, ExecutionErrorKind},
};

use super::RuntimeExecution;

pub(in crate::execution::docker::backend::lifecycle) async fn verify_owned(
    backend: &DockerExecutionBackend,
    id: &str,
    execution_id: &str,
) -> Result<(), ExecutionError> {
    let info = tokio::time::timeout(
        Duration::from_secs(5),
        backend.docker.inspect_container(
            id,
            Some(
                InspectContainerOptionsBuilder::default()
                    .size(false)
                    .build(),
            ),
        ),
    )
    .await
    .map_err(|_| operation_timeout())?
    .map_err(|error| operation_disconnected("verify execution container ownership", &error))?;
    let labels = info
        .config
        .and_then(|config| config.labels)
        .unwrap_or_default();
    if labels.get("com.koolade.owner").map(String::as_str)
        != Some(&backend.identity.owner_instance_id)
        || labels.get("com.koolade.execution").map(String::as_str) != Some(execution_id)
    {
        return Err(ExecutionError::new(
            ExecutionErrorKind::PolicyDenied,
            "Container ownership labels do not match this execution",
            false,
        ));
    }
    Ok(())
}

pub(in crate::execution::docker::backend::lifecycle) fn operation_timeout() -> ExecutionError {
    ExecutionError::new(
        ExecutionErrorKind::WorkerDisconnected,
        "Docker Engine operation exceeded its bounded deadline",
        true,
    )
}

pub(in crate::execution::docker::backend::lifecycle) fn terminal(
    execution: &Arc<Mutex<RuntimeExecution>>,
    state_kind: ExecutionLifecycleV1,
    failure: Option<ExecutionError>,
    code: Option<i32>,
    timed_out: bool,
    cancelled: bool,
) -> Result<(), ExecutionError> {
    let Ok(mut state) = execution.lock() else {
        return Err(
            crate::execution::docker::backend::internal_error_with_context(
                "recording terminal execution state",
            ),
        );
    };
    if state.terminal {
        return Ok(());
    }
    state.mark_collecting()?;
    state.terminal = true;
    state.host_capacity_lease.take();
    let mut terminal_result = result(
        &state.request.execution_id,
        failure,
        code,
        timed_out,
        cancelled,
    );
    terminal_result.artifacts = state.artifacts.clone();
    state.result = Some(terminal_result);
    state.push(ExecutionEventKindV1::Lifecycle { state: state_kind })?;
    Ok(())
}

pub(in crate::execution::docker::backend::lifecycle) fn result(
    execution_id: &str,
    failure: Option<ExecutionError>,
    code: Option<i32>,
    timed_out: bool,
    cancelled: bool,
) -> ExecutionResultV1 {
    ExecutionResultV1 {
        schema_version: 1,
        execution_id: execution_id.into(),
        exit_code: code,
        signal: None,
        timed_out,
        cancelled,
        metrics: ResourceMetricsV1 {
            cpu_millis: 0,
            peak_memory_bytes: 0,
            process_count: 0,
            disk_bytes: 0,
        },
        phase_timings: Default::default(),
        artifacts: Vec::new(),
        failure,
    }
}

pub(in crate::execution::docker::backend::lifecycle) fn disconnected() -> ExecutionError {
    ExecutionError::new(
        ExecutionErrorKind::WorkerDisconnected,
        "Docker Engine operation failed or disconnected",
        true,
    )
}

pub(in crate::execution::docker::backend::lifecycle) fn operation_disconnected(
    operation: &str,
    error: &bollard::errors::Error,
) -> ExecutionError {
    let detail = match error {
        bollard::errors::Error::DockerResponseServerError { status_code, .. } => {
            format!("Docker API returned HTTP {status_code}")
        }
        bollard::errors::Error::RequestTimeoutError => "Docker API request timed out".into(),
        bollard::errors::Error::DockerStreamError { .. } => "Docker stream was interrupted".into(),
        _ => "Docker Engine connection failed".into(),
    };
    ExecutionError::new(
        ExecutionErrorKind::WorkerDisconnected,
        format!("Docker could not {operation}: {detail}"),
        true,
    )
}

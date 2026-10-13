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
    .map_err(|_| disconnected())?;
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
        return Err(super::internal_error());
    };
    if state.terminal {
        return Ok(());
    }
    state.push(ExecutionEventKindV1::Lifecycle {
        state: ExecutionLifecycleV1::Collecting,
    })?;
    state.terminal = true;
    state.result = Some(result(
        &state.request.execution_id,
        failure,
        code,
        timed_out,
        cancelled,
    ));
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

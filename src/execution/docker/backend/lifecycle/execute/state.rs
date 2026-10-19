use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};

use super::super::runtime::RuntimeExecution;
use crate::execution::{
    contract::{ExecutionEventKindV1, ExecutionLifecycleV1},
    errors::{ExecutionError, ExecutionErrorKind},
};

#[cfg(test)]
pub(super) const EXEC_CREATED: u8 = 1 << 0;
#[cfg(test)]
pub(super) const EXEC_ATTACHED: u8 = 1 << 1;
#[cfg(test)]
pub(super) const INPUT_WRITTEN: u8 = 1 << 2;
#[cfg(test)]
pub(super) const INPUT_CLOSED: u8 = 1 << 3;
#[cfg(test)]
pub(super) const OUTPUT_RECEIVED: u8 = 1 << 4;

#[cfg(test)]
pub(super) fn mark_diagnostic_progress(execution: &Arc<Mutex<RuntimeExecution>>, progress: u8) {
    if let Ok(state) = execution.lock() {
        state
            .diagnostic_progress
            .fetch_or(progress, std::sync::atomic::Ordering::Relaxed);
    }
}

pub(super) fn finish(
    execution: &Arc<Mutex<RuntimeExecution>>,
    lifecycle: ExecutionLifecycleV1,
    failure: Option<ExecutionError>,
    code: Option<i32>,
    timed_out: bool,
    cancelled: bool,
) -> Result<(), ExecutionError> {
    finish_with_artifacts(
        execution,
        lifecycle,
        failure,
        code,
        timed_out,
        cancelled,
        Vec::new(),
    )
}

pub(super) fn finish_with_artifacts(
    execution: &Arc<Mutex<RuntimeExecution>>,
    lifecycle: ExecutionLifecycleV1,
    failure: Option<ExecutionError>,
    code: Option<i32>,
    timed_out: bool,
    cancelled: bool,
    artifacts: Vec<crate::execution::contract::ArtifactManifestEntryV1>,
) -> Result<(), ExecutionError> {
    let Ok(mut state) = execution.lock() else {
        return Err(disconnected());
    };
    if state.terminal || state.cancellation_requests.load(Ordering::Acquire) > 0 {
        return Ok(());
    }
    state.mark_collecting()?;
    state.terminal = true;
    state.host_capacity_lease.take();
    let mut result = super::super::control::result(
        &state.request.execution_id,
        failure,
        code,
        timed_out,
        cancelled,
    );
    if !artifacts.is_empty() {
        state.artifacts = artifacts;
    }
    result.artifacts = state.artifacts.clone();
    state.result = Some(result);
    state.push(ExecutionEventKindV1::Lifecycle { state: lifecycle })?;
    Ok(())
}

pub(super) fn mark_collecting(
    execution: &Arc<Mutex<RuntimeExecution>>,
) -> Result<bool, ExecutionError> {
    let mut state = execution.lock().map_err(|_| disconnected())?;
    if state.terminal || state.cancellation_requests.load(Ordering::Acquire) > 0 {
        return Ok(false);
    }
    state.mark_collecting().map(|()| true)
}

pub(in crate::execution::docker::backend::lifecycle) fn persist_stage_out(
    execution: &Arc<Mutex<RuntimeExecution>>,
    elapsed_ms: u64,
    artifacts: Option<&[crate::execution::contract::ArtifactManifestEntryV1]>,
) -> Result<(), ExecutionError> {
    let mut state = execution.lock().map_err(|_| {
        crate::execution::docker::backend::internal_error_with_context(
            "recording workspace synchronization telemetry",
        )
    })?;
    if state.terminal {
        return Ok(());
    }
    state.phase_timings.workspace_stage_out_ms = Some(elapsed_ms);
    if let Some(artifacts) = artifacts {
        state.artifacts = artifacts.to_vec();
    }
    state.persist()
}

pub(super) fn interrupted(execution: &Arc<Mutex<RuntimeExecution>>) -> Result<(), ExecutionError> {
    interrupted_with(execution, disconnected())
}

pub(super) fn interrupted_with(
    execution: &Arc<Mutex<RuntimeExecution>>,
    error: ExecutionError,
) -> Result<(), ExecutionError> {
    finish(
        execution,
        ExecutionLifecycleV1::Interrupted,
        Some(error.clone()),
        None,
        false,
        false,
    )?;
    Err(error)
}

pub(super) fn output_limit_error() -> ExecutionError {
    ExecutionError::new(
        ExecutionErrorKind::ExecutionFailed,
        "Command output exceeded its policy limit",
        false,
    )
}

pub(super) fn disconnected() -> ExecutionError {
    ExecutionError::new(
        ExecutionErrorKind::WorkerDisconnected,
        "Docker Engine operation failed or disconnected",
        true,
    )
}

pub(super) fn operation_disconnected(
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

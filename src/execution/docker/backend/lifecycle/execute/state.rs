use std::sync::{Arc, Mutex};

use super::super::runtime::RuntimeExecution;
use crate::execution::{
    contract::{ExecutionEventKindV1, ExecutionLifecycleV1},
    errors::{ExecutionError, ExecutionErrorKind},
};

pub(super) fn finish(
    execution: &Arc<Mutex<RuntimeExecution>>,
    lifecycle: ExecutionLifecycleV1,
    failure: Option<ExecutionError>,
    code: Option<i32>,
    timed_out: bool,
    cancelled: bool,
) -> Result<(), ExecutionError> {
    let Ok(mut state) = execution.lock() else {
        return Err(disconnected());
    };
    if state.terminal {
        return Ok(());
    }
    state.push(ExecutionEventKindV1::Lifecycle {
        state: ExecutionLifecycleV1::Collecting,
    })?;
    state.terminal = true;
    state.result = Some(super::super::control::result(
        &state.request.execution_id,
        failure,
        code,
        timed_out,
        cancelled,
    ));
    state.push(ExecutionEventKindV1::Lifecycle { state: lifecycle })?;
    Ok(())
}

pub(super) fn interrupted(execution: &Arc<Mutex<RuntimeExecution>>) -> Result<(), ExecutionError> {
    let error = disconnected();
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

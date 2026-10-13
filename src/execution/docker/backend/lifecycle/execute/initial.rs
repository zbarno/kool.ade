use std::sync::{Arc, Mutex};

use crate::execution::{
    backend::ExecutionHandleV1,
    contract::ExecutionRequestV1,
    docker::{
        backend::DockerExecutionBackend,
        sandbox::{ValidatedWorkspace, WorkspaceSnapshot},
    },
    errors::ExecutionError,
};
use bollard::exec::CreateExecOptions;

pub(super) type InitialState = (
    Arc<Mutex<super::super::runtime::RuntimeExecution>>,
    String,
    u64,
    u64,
    Result<(), ExecutionError>,
    ValidatedWorkspace,
    Option<WorkspaceSnapshot>,
);

pub(super) fn begin(
    backend: &DockerExecutionBackend,
    handle: &ExecutionHandleV1,
    request: &ExecutionRequestV1,
) -> Result<InitialState, ExecutionError> {
    let execution = backend.lookup_execution(handle)?;
    let mut state = execution
        .lock()
        .map_err(|_| crate::execution::docker::backend::internal_error())?;
    if state.request != *request || state.executed || state.terminal {
        return Err(crate::execution::docker::backend::invalid_request());
    }
    state.executed = true;
    state.command_started = true;
    let saved_attempt = state.persist();
    let container_id = state.container_id.clone();
    let budget = state.request.resource_budget.output_bytes;
    let deadline = state.request.deadline_unix_ms;
    let workspace = state._workspace.clone();
    let snapshot = state.workspace_snapshot.clone();
    drop(state);
    Ok((
        execution,
        container_id,
        budget,
        deadline,
        saved_attempt,
        workspace,
        snapshot,
    ))
}

pub(super) fn command_config(request: &ExecutionRequestV1) -> CreateExecOptions<String> {
    CreateExecOptions {
        attach_stdout: Some(true),
        attach_stderr: Some(true),
        attach_stdin: Some(request.stdin.is_some()),
        tty: Some(false),
        cmd: Some(request.argv.clone()),
        user: Some("10001:10001".into()),
        working_dir: Some(format!("/workspace/{}", request.working_directory_relative)),
        privileged: Some(false),
        ..Default::default()
    }
}

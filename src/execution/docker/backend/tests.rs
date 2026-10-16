use super::{handle_matches, internal_error_with_detail};
use crate::execution::{
    backend::ExecutionHandleV1, contract::ExecutionTarget, errors::ExecutionErrorKind,
};

#[cfg(unix)]
#[path = "tests/docker_engine.rs"]
mod docker_engine;

fn handle() -> ExecutionHandleV1 {
    ExecutionHandleV1 {
        execution_id: "execution-a".into(),
        task_id: "task-a".into(),
        target: ExecutionTarget::local(),
        request_sequence: 3,
    }
}

#[test]
fn execution_handle_is_bound_to_all_record_identity_fields() {
    let handle = handle();
    assert!(handle_matches(
        &handle,
        "execution-a",
        "task-a",
        &ExecutionTarget::local(),
        3,
    ));
    assert!(!handle_matches(
        &handle,
        "execution-b",
        "task-a",
        &ExecutionTarget::local(),
        3,
    ));
    assert!(!handle_matches(
        &handle,
        "execution-a",
        "task-b",
        &ExecutionTarget::local(),
        3,
    ));
    assert!(!handle_matches(
        &handle,
        "execution-a",
        "task-a",
        &ExecutionTarget::remote("worker-a"),
        3,
    ));
    assert!(!handle_matches(
        &handle,
        "execution-a",
        "task-a",
        &ExecutionTarget::local(),
        4,
    ));
}

#[test]
fn internal_state_error_preserves_bounded_control_free_diagnostics() {
    let detail = format!("event [31m{}", "x".repeat(2_000));
    let error = internal_error_with_detail("reading execution events", &detail);

    assert_eq!(error.kind, ExecutionErrorKind::WorkerDisconnected);
    assert!(error.retryable);
    assert!(error.message.starts_with(
        "Execution state could not be accessed while reading execution events: event [31m"
    ));
    assert!(error.message.len() < 2_100);
    error.validate().unwrap();
}

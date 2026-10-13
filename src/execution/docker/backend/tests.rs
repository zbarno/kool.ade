use super::handle_matches;
use crate::execution::{backend::ExecutionHandleV1, contract::ExecutionTarget};

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

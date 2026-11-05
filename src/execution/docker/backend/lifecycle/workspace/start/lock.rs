use crate::execution::{
    docker::backend::internal_error_with_context,
    errors::{ExecutionError, ExecutionErrorKind},
};

/// Acquire the same durable task lock used by Docker execution admission.
/// Queue recovery tests exercise this production fence without a Docker daemon.
pub(crate) fn acquire_task_execution_lock(task_id: &str) -> Result<std::fs::File, ExecutionError> {
    crate::persistence::execution_records::try_task_lock(task_id)
        .map_err(|_| internal_error_with_context("acquiring the task execution lock"))?
        .ok_or_else(|| {
            ExecutionError::new(
                ExecutionErrorKind::PolicyDenied,
                "Another execution already owns this task clone",
                false,
            )
        })
}

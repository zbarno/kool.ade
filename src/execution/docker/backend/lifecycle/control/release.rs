use std::time::Duration;

use bollard::query_parameters::InspectContainerOptionsBuilder;

use crate::execution::{
    backend::ExecutionHandleV1,
    docker::backend::DockerExecutionBackend,
    errors::{ExecutionError, ExecutionErrorKind},
};
use crate::persistence::execution_records::{self, ExecutionCleanupStateV1};

pub(super) async fn release_persisted(
    backend: &DockerExecutionBackend,
    handle: &ExecutionHandleV1,
) -> Result<(), ExecutionError> {
    let mut record = super::load_record(backend, handle)?;
    if !is_terminal(record.state) {
        return Err(ExecutionError::new(
            ExecutionErrorKind::PolicyDenied,
            "Running executions cannot be released",
            false,
        ));
    }
    if record.cleanup != ExecutionCleanupStateV1::Complete {
        let container_id = record
            .container_id
            .as_deref()
            .or(record.container_name.as_deref())
            .ok_or_else(super::disconnected)?;
        let inspected = tokio::time::timeout(
            Duration::from_secs(5),
            backend.docker.inspect_container(
                container_id,
                Some(
                    InspectContainerOptionsBuilder::default()
                        .size(false)
                        .build(),
                ),
            ),
        )
        .await
        .map_err(|_| super::operation_timeout())?;
        match inspected {
            Ok(info) => {
                super::super::recovery::verify_record_labels(
                    &record,
                    &info
                        .config
                        .and_then(|config| config.labels)
                        .unwrap_or_default(),
                )?;
                tokio::time::timeout(
                    Duration::from_secs(5),
                    backend.docker.remove_container(
                        container_id,
                        Some(bollard::query_parameters::RemoveContainerOptions {
                            force: false,
                            ..Default::default()
                        }),
                    ),
                )
                .await
                .map_err(|_| super::operation_timeout())?
                .map_err(|_| super::disconnected())?;
            }
            Err(bollard::errors::Error::DockerResponseServerError {
                status_code: 404, ..
            }) => {}
            Err(_) => return Err(super::disconnected()),
        }
        record.cleanup = ExecutionCleanupStateV1::Complete;
        record.updated_at_unix_ms =
            crate::execution::docker::backend::now_unix_ms().max(record.started_at_unix_ms);
        execution_records::save(&record).map_err(|_| super::internal_error())?;
    }
    if let Some(lease_id) = record.workspace_lease_id.as_deref() {
        let prepared = backend
            .workspaces
            .lock()
            .map_err(|_| super::internal_error())?
            .get(lease_id)
            .cloned();
        if let Some(prepared) = prepared {
            let mut state = prepared.lock().map_err(|_| super::internal_error())?;
            if state.active_execution_id.as_deref() != Some(handle.execution_id.as_str()) {
                return Err(ExecutionError::new(
                    ExecutionErrorKind::PolicyDenied,
                    "Workspace lease belongs to a different execution",
                    false,
                ));
            }
            state.active_execution_id = None;
            drop(state);
            backend
                .workspaces
                .lock()
                .map_err(|_| super::internal_error())?
                .remove(lease_id);
        }
    }
    backend
        .executions
        .lock()
        .map_err(|_| super::internal_error())?
        .remove(&handle.execution_id);
    Ok(())
}

fn is_terminal(state: execution_records::ExecutionRecordStateV1) -> bool {
    matches!(
        state,
        execution_records::ExecutionRecordStateV1::Succeeded
            | execution_records::ExecutionRecordStateV1::Failed
            | execution_records::ExecutionRecordStateV1::Cancelled
            | execution_records::ExecutionRecordStateV1::Interrupted
    )
}

use crate::execution::errors::ExecutionError;

use super::{RemoteExecutionBackend, disconnected};

mod recovery;
mod stage;
pub(super) use recovery::{
    ActivePrivateFileExecution, active_execution, bind_start_request, mark_stage_complete,
    recover_execution_grants_at, save_execution_grants,
};
pub(super) use stage::{select_for_execution, stage_for_execution};

pub(super) fn grants_are_active(
    grants: &[crate::execution::remote::private_files::RemotePrivateFileGrantV1],
) -> bool {
    let root = crate::persistence::state_root();
    let now = crate::persistence::harness_settings::current_unix_ms();
    grants_are_active_at(&root, grants, now)
}

fn grants_are_active_at(
    root: &std::path::Path,
    grants: &[crate::execution::remote::private_files::RemotePrivateFileGrantV1],
    now: u64,
) -> bool {
    grants.iter().all(|grant| {
        crate::persistence::remote_private_file_grants::require_authorized(root, grant, now).is_ok()
    })
}

pub(super) fn revoked_execution_generation(
    backend: &RemoteExecutionBackend,
    handle: &crate::execution::backend::ExecutionHandleV1,
) -> Result<Option<u64>, ExecutionError> {
    let Some(active) = active_execution(backend, &handle.execution_id)? else {
        return Ok(None);
    };
    if active.task_id != handle.task_id || active.request_sequence != handle.request_sequence {
        return Err(incompatible());
    }
    Ok((!grants_are_active(&active.grants)).then_some(active.cancellation_generation))
}

pub(super) fn revoked_error() -> ExecutionError {
    ExecutionError::new(
        crate::execution::errors::ExecutionErrorKind::PolicyDenied,
        "Remote private-file approval was revoked; the worker execution was stopped",
        false,
    )
}

#[cfg(test)]
#[path = "private_files/tests.rs"]
mod tests;

impl RemoteExecutionBackend {
    pub(super) fn lock_execution_private_file_grants(
        &self,
        execution_id: &str,
    ) -> Result<
        crate::persistence::remote_private_file_grants::AuthorizedPrivateFileGrantUse,
        ExecutionError,
    > {
        let grants = self
            .execution_private_file_grants
            .lock()
            .map_err(|_| disconnected())?
            .get(execution_id)
            .map(|active| active.grants.clone())
            .unwrap_or_default();
        crate::persistence::remote_private_file_grants::lock_authorized_grants(
            &crate::persistence::state_root(),
            &grants,
            crate::persistence::harness_settings::current_unix_ms(),
        )
        .map_err(|_| revoked_error())
    }

    pub(super) async fn stop_if_private_grant_revoked(
        &self,
        handle: &crate::execution::backend::ExecutionHandleV1,
    ) -> Result<(), ExecutionError> {
        if self
            .revoked_private_file_executions
            .lock()
            .map_err(|_| disconnected())?
            .contains(&handle.execution_id)
        {
            return Err(revoked_error());
        }
        let Some(cancellation_generation) = revoked_execution_generation(self, handle)? else {
            return Ok(());
        };
        discard_verified_result(self, &handle.execution_id)?;
        let local_handle = self.local_handle(handle)?;
        let worker_id = match &self.target.location {
            crate::execution::contract::TargetLocation::Remote { worker_id } => worker_id,
            crate::execution::contract::TargetLocation::Local => return Err(incompatible()),
        };
        let cancellation: Result<serde_json::Value, ExecutionError> = self.request(
            crate::execution::remote::protocol::EnvelopeKindV1::Cancel,
            &handle.execution_id,
            serde_json::json!({"handle": local_handle.clone(),
                "cancellation_generation": cancellation_generation}),
        );
        if let Err(cancel_error) = cancellation {
            let abort: Result<serde_json::Value, ExecutionError> = self.request(
                crate::execution::remote::protocol::EnvelopeKindV1::AbortPrivateFileStage,
                &handle.execution_id,
                serde_json::json!({"execution_id": handle.execution_id}),
            );
            if abort.is_err() {
                return Err(cancel_error);
            }
            self.client
                .lock()
                .map_err(|_| disconnected())?
                .deny_pending_start(&handle.execution_id)?;
            forget_revoked_private_file_execution(self, handle, worker_id)?;
            return Err(revoked_error());
        }
        let _: serde_json::Value = self.request(
            crate::execution::remote::protocol::EnvelopeKindV1::Release,
            &handle.execution_id,
            serde_json::json!({"handle": local_handle}),
        )?;
        self.client
            .lock()
            .map_err(|_| disconnected())?
            .deny_pending_start(&handle.execution_id)?;
        forget_revoked_private_file_execution(self, handle, worker_id)?;
        Err(revoked_error())
    }
}

fn forget_revoked_private_file_execution(
    backend: &RemoteExecutionBackend,
    handle: &crate::execution::backend::ExecutionHandleV1,
    worker_id: &str,
) -> Result<(), ExecutionError> {
    backend
        .revoked_private_file_executions
        .lock()
        .map_err(|_| disconnected())?
        .insert(handle.execution_id.clone());
    backend
        .execution_private_file_grants
        .lock()
        .map_err(|_| disconnected())?
        .remove(&handle.execution_id);
    crate::persistence::remote_private_file_executions::remove(
        &crate::persistence::state_root(),
        worker_id,
        &handle.execution_id,
    )
    .map_err(|_| disconnected())?;
    backend
        .execution_snapshots
        .lock()
        .map_err(|_| disconnected())?
        .remove(&handle.execution_id);
    Ok(())
}

fn discard_verified_result(
    backend: &RemoteExecutionBackend,
    execution_id: &str,
) -> Result<(), ExecutionError> {
    let result = backend
        .verified_results
        .lock()
        .map_err(|_| disconnected())?
        .get(execution_id)
        .cloned();
    if let Some(result) = result {
        remove_result_transfer(&backend.result_root, result.transfer_id())?;
        backend
            .verified_results
            .lock()
            .map_err(|_| disconnected())?
            .remove(execution_id);
    }
    Ok(())
}

pub(super) fn remove_result_transfer(
    root: &std::path::Path,
    transfer_id: &str,
) -> Result<(), ExecutionError> {
    let parsed = uuid::Uuid::parse_str(transfer_id).map_err(|_| incompatible())?;
    if parsed.hyphenated().to_string() != transfer_id {
        return Err(incompatible());
    }
    let root = root.canonicalize().map_err(|_| disconnected())?;
    let directory = root.join(transfer_id);
    let metadata = match std::fs::symlink_metadata(&directory) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(_) => return Err(incompatible()),
    };
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        return Err(incompatible());
    }
    let canonical = directory.canonicalize().map_err(|_| incompatible())?;
    if !canonical.starts_with(&root) {
        return Err(incompatible());
    }
    std::fs::remove_dir_all(canonical).map_err(|_| disconnected())
}

fn incompatible() -> ExecutionError {
    ExecutionError::new(
        crate::execution::errors::ExecutionErrorKind::PolicyDenied,
        "Remote private-file approval is no longer valid for this worker and task",
        false,
    )
}

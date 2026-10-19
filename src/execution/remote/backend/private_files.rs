use crate::execution::{
    contract::ExecutionRequestV1,
    docker::backend::{OwnedTaskWorkspaceResolver, TaskWorkspaceResolver},
    errors::ExecutionError,
};

use super::super::{WorkspaceManifestV1, client::RemoteWorkerClient};
use super::{RemoteExecutionBackend, disconnected};

pub(super) fn stage_for_execution(
    backend: &RemoteExecutionBackend,
    client: &mut RemoteWorkerClient,
    request: &ExecutionRequestV1,
    snapshot: &WorkspaceManifestV1,
) -> Result<Vec<crate::execution::remote::private_files::RemotePrivateFileGrantV1>, ExecutionError>
{
    use crate::execution::remote::protocol::EnvelopeKindV1 as Kind;

    let worker_id = match &backend.target.location {
        crate::execution::contract::TargetLocation::Remote { worker_id } => worker_id,
        crate::execution::contract::TargetLocation::Local => return Err(incompatible()),
    };
    let root = crate::persistence::state_root();
    let grants = crate::persistence::remote_private_file_grants::active_for_task(
        &root,
        &request.workspace.project_id,
        &request.workspace.task_id,
        crate::persistence::harness_settings::current_unix_ms(),
    )
    .map_err(|_| incompatible())?;
    let Some(source_root) = backend.source_root.as_deref() else {
        if grants
            .iter()
            .any(|grant| grant.workspace == request.workspace)
        {
            return Err(incompatible());
        }
        return Ok(Vec::new());
    };
    let resolver =
        OwnedTaskWorkspaceResolver::for_current_user(request.workspace.clone(), source_root)?;
    let clone_path = resolver.resolve_owned_clone(&request.workspace)?;
    let mut selected_grants = Vec::new();
    let transfer = (|| {
        for grant in grants
            .into_iter()
            .filter(|grant| grant.workspace == request.workspace)
        {
            if grant.controller_id != backend.controller_id || grant.worker_id != *worker_id {
                // Receipts for another controller or destination do not grant
                // authority here and must not block a separate valid target.
                continue;
            }
            if grant.worker_host_key_fingerprint != client.verified_host_key_fingerprint() {
                return Err(incompatible());
            }
            selected_grants.push(grant.clone());
            client.request(
                Kind::AuthorizePrivateFileGrant,
                &request.execution_id,
                serde_json::json!({"execution_id": request.execution_id,
                    "workspace": request.workspace, "grant": grant}),
            )?;
            for spec in &grant.files {
                let source = crate::execution::remote::private_files::read_authorized_private_file(
                    &root,
                    &clone_path,
                    &grant,
                    &spec.path,
                    snapshot,
                    crate::persistence::harness_settings::current_unix_ms(),
                )
                .map_err(|_| incompatible())?;
                client.send_authorized_private_file(
                    &request.execution_id,
                    &grant,
                    &spec.path,
                    &source,
                )?;
            }
        }
        Ok(())
    })();
    if transfer.is_err() {
        // This execution has not received Start from the controller yet. The
        // worker refuses this cleanup if its stage is already mounted, which
        // protects an earlier ambiguous Start using the same execution ID.
        let _ = client.request(
            Kind::AbortPrivateFileStage,
            &request.execution_id,
            serde_json::json!({"execution_id": request.execution_id}),
        );
    }
    transfer.map(|()| selected_grants)
}

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
    execution_id: &str,
) -> Result<Option<u64>, ExecutionError> {
    let runs = backend
        .execution_private_file_grants
        .lock()
        .map_err(|_| disconnected())?;
    let Some((grants, cancellation_generation)) = runs.get(execution_id) else {
        return Ok(None);
    };
    Ok((!grants_are_active(grants)).then_some(*cancellation_generation))
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
            .map(|(grants, _)| grants.clone())
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
        let Some(cancellation_generation) =
            revoked_execution_generation(self, &handle.execution_id)?
        else {
            return Ok(());
        };
        discard_verified_result(self, &handle.execution_id)?;
        let local_handle = self.local_handle(handle)?;
        let _: serde_json::Value = self.request(
            crate::execution::remote::protocol::EnvelopeKindV1::Cancel,
            &handle.execution_id,
            serde_json::json!({"handle": local_handle.clone(),
                "cancellation_generation": cancellation_generation}),
        )?;
        let _: serde_json::Value = self.request(
            crate::execution::remote::protocol::EnvelopeKindV1::Release,
            &handle.execution_id,
            serde_json::json!({"handle": local_handle}),
        )?;
        self.revoked_private_file_executions
            .lock()
            .map_err(|_| disconnected())?
            .insert(handle.execution_id.clone());
        self.execution_private_file_grants
            .lock()
            .map_err(|_| disconnected())?
            .remove(&handle.execution_id);
        self.execution_snapshots
            .lock()
            .map_err(|_| disconnected())?
            .remove(&handle.execution_id);
        Err(revoked_error())
    }
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

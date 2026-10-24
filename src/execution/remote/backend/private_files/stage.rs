use std::path::PathBuf;

use crate::execution::{
    contract::ExecutionRequestV1,
    docker::backend::{OwnedTaskWorkspaceResolver, TaskWorkspaceResolver},
    errors::ExecutionError,
    remote::private_files::RemotePrivateFileGrantV1,
};

use super::super::super::{WorkspaceManifestV1, client::RemoteWorkerClient};
use super::super::RemoteExecutionBackend;
use super::incompatible;

pub(in crate::execution::remote::backend) struct SelectedPrivateFileGrants {
    pub(in crate::execution::remote::backend) grants: Vec<RemotePrivateFileGrantV1>,
    clone_path: Option<PathBuf>,
}

/// Select and validate local source paths before persisting an execution link
/// or sending any grant metadata or file bytes to the worker.
pub(in crate::execution::remote::backend) fn select_for_execution(
    backend: &RemoteExecutionBackend,
    client: &RemoteWorkerClient,
    request: &ExecutionRequestV1,
) -> Result<SelectedPrivateFileGrants, ExecutionError> {
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
        return Ok(SelectedPrivateFileGrants {
            grants: Vec::new(),
            clone_path: None,
        });
    };
    let resolver =
        OwnedTaskWorkspaceResolver::for_current_user(request.workspace.clone(), source_root)?;
    let clone_path = resolver.resolve_owned_clone(&request.workspace)?;
    let mut selected = Vec::new();
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
        selected.push(grant);
    }
    Ok(SelectedPrivateFileGrants {
        grants: selected,
        clone_path: Some(clone_path),
    })
}

pub(in crate::execution::remote::backend) fn stage_for_execution(
    client: &mut RemoteWorkerClient,
    request: &ExecutionRequestV1,
    snapshot: &WorkspaceManifestV1,
    selected: &SelectedPrivateFileGrants,
) -> Result<(), ExecutionError> {
    use crate::execution::remote::protocol::EnvelopeKindV1 as Kind;

    let root = crate::persistence::state_root();
    let Some(clone_path) = selected.clone_path.as_deref() else {
        return if selected.grants.is_empty() {
            Ok(())
        } else {
            Err(incompatible())
        };
    };
    let transfer = (|| {
        for grant in &selected.grants {
            {
                // Serialize the worker authorization metadata with revocation.
                // File reads revalidate the receipt, and the chunk sender holds
                // the same lock around each secret-bearing frame.
                let _grant_use =
                    crate::persistence::remote_private_file_grants::lock_authorized_grants(
                        &root,
                        std::slice::from_ref(grant),
                        crate::persistence::harness_settings::current_unix_ms(),
                    )
                    .map_err(|_| incompatible())?;
                client.request(
                    Kind::AuthorizePrivateFileGrant,
                    &request.execution_id,
                    serde_json::json!({"execution_id": request.execution_id,
                        "workspace": request.workspace, "grant": grant}),
                )?;
            }
            for spec in &grant.files {
                let source = crate::execution::remote::private_files::read_authorized_private_file(
                    &root,
                    clone_path,
                    grant,
                    &spec.path,
                    snapshot,
                    crate::persistence::harness_settings::current_unix_ms(),
                )
                .map_err(|_| incompatible())?;
                client.send_authorized_private_file(
                    &request.execution_id,
                    grant,
                    &spec.path,
                    &source,
                )?;
            }
        }
        Ok(())
    })();
    if transfer.is_err() {
        // The worker refuses this cleanup if the stage is already mounted,
        // protecting an earlier ambiguous Start with the same execution ID.
        let _ = client.request(
            Kind::AbortPrivateFileStage,
            &request.execution_id,
            serde_json::json!({"execution_id": request.execution_id}),
        );
    }
    transfer
}

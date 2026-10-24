use sha2::Digest;

use crate::execution::{
    backend::{BackendFuture, ExecutionHandleV1, WorkspaceLeaseV1},
    contract::{ExecutionRequestV1, ExecutionTarget, TargetLocation},
};

use super::super::super::protocol::EnvelopeKindV1;
use super::super::{RemoteExecutionBackend, disconnected, incompatible, private_files};

pub(super) fn start<'a>(
    backend: &'a RemoteExecutionBackend,
    lease: &'a WorkspaceLeaseV1,
    request: &'a ExecutionRequestV1,
) -> BackendFuture<'a, ExecutionHandleV1> {
    Box::pin(async move {
        backend.require_target(&request.target)?;
        if backend
            .revoked_private_file_executions
            .lock()
            .map_err(|_| disconnected())?
            .contains(&request.execution_id)
        {
            return Err(private_files::revoked_error());
        }
        if lease.workspace != request.workspace {
            return Err(incompatible());
        }
        let mut worker_request = request.clone();
        worker_request.controller_target_id = Some(backend.target.target_id.clone());
        worker_request.target = ExecutionTarget::local();
        let snapshot = backend
            .snapshots
            .lock()
            .map_err(|_| disconnected())?
            .get(&request.workspace)
            .cloned()
            .ok_or_else(incompatible)?;
        let start_payload = serde_json::json!({"lease": lease, "request": worker_request});
        let start_payload_sha256 = format!(
            "sha256:{:x}",
            sha2::Sha256::digest(serde_json::to_vec(&start_payload).map_err(|_| incompatible())?)
        );
        let start_result = {
            let mut client = backend.client.lock().map_err(|_| disconnected())?;
            let selected = private_files::select_for_execution(backend, &client, request)?;
            let private_grants = selected.grants.clone();
            let mut active = private_files::active_execution(backend, &request.execution_id)?;
            let prior_link = active.is_some();
            let active_grants_authorized = active
                .as_ref()
                .is_none_or(|existing| private_files::grants_are_active(&existing.grants));
            if let Some(existing) = &active
                && (existing.task_id != request.task_id
                    || existing.request_sequence != request.request_sequence
                    || existing.cancellation_generation != request.cancellation_generation
                    || (active_grants_authorized
                        && existing.start_payload_sha256 != start_payload_sha256))
            {
                return Err(incompatible());
            }
            let revoked_handle = if let Some(existing) = &active {
                if existing.grants != private_grants && active_grants_authorized {
                    return Err(incompatible());
                }
                if !active_grants_authorized {
                    if existing.start_request_sequence.is_some() {
                        Some(ExecutionHandleV1 {
                            execution_id: request.execution_id.clone(),
                            task_id: existing.task_id.clone(),
                            target: backend.target.clone(),
                            request_sequence: existing.request_sequence,
                        })
                    } else {
                        client.request(
                            EnvelopeKindV1::AbortPrivateFileStage,
                            &request.execution_id,
                            serde_json::json!({"execution_id": request.execution_id}),
                        )?;
                        remove_unstarted_grant_link(backend, &request.execution_id)?;
                        return Err(private_files::revoked_error());
                    }
                } else {
                    None
                }
            } else if !private_grants.is_empty() {
                let worker_id = match &backend.target.location {
                    TargetLocation::Remote { worker_id } => worker_id,
                    TargetLocation::Local => return Err(incompatible()),
                };
                let saved_active = match private_files::save_execution_grants(
                    &crate::persistence::state_root(),
                    request,
                    worker_id,
                    &backend.controller_id,
                    &private_grants,
                    &start_payload_sha256,
                ) {
                    Ok(active) => active,
                    Err(error) => {
                        let _ = client.request(
                            EnvelopeKindV1::AbortPrivateFileStage,
                            &request.execution_id,
                            serde_json::json!({"execution_id": request.execution_id}),
                        );
                        return Err(error);
                    }
                };
                backend
                    .execution_private_file_grants
                    .lock()
                    .map_err(|_| disconnected())?
                    .insert(request.execution_id.clone(), saved_active.clone());
                active = Some(saved_active);
                None
            } else {
                None
            };
            if let Some(handle) = revoked_handle {
                (handle, false, std::time::Instant::now(), true)
            } else {
                let replaying_start = active
                    .as_ref()
                    .is_some_and(|execution| execution.start_request_sequence.is_some());
                if let Some(execution) = active.as_ref() {
                    if execution.start_request_sequence.is_none() && !execution.stage_complete {
                        if prior_link {
                            client.request(
                                EnvelopeKindV1::AbortPrivateFileStage,
                                &request.execution_id,
                                serde_json::json!({"execution_id": request.execution_id}),
                            )?;
                        }
                        private_files::stage_for_execution(
                            &mut client,
                            request,
                            &snapshot,
                            &selected,
                        )?;
                        private_files::mark_stage_complete(backend, &request.execution_id)?;
                    }
                } else {
                    private_files::stage_for_execution(&mut client, request, &snapshot, &selected)?;
                }
                let start_sequence = active
                    .as_ref()
                    .and_then(|execution| execution.start_request_sequence)
                    .unwrap_or_else(|| client.next_sequence_for(&request.execution_id));
                if active.is_some() {
                    client.prepare_start_request(&request.execution_id, start_sequence)?;
                }
                if let Some(execution) = active.as_mut()
                    && execution.start_request_sequence.is_none()
                {
                    private_files::bind_start_request(
                        backend,
                        &request.execution_id,
                        start_sequence,
                    )?;
                    execution.start_request_sequence = Some(start_sequence);
                }
                // A Start frame written to this verified stream is an in-flight
                // authorized use. The lock releases after that write; a later
                // revocation is canceled before the handle reaches the caller.
                let grant_use =
                    crate::persistence::remote_private_file_grants::lock_authorized_grants(
                        &crate::persistence::state_root(),
                        &private_grants,
                        crate::persistence::harness_settings::current_unix_ms(),
                    )
                    .map_err(|_| disconnected())?;
                let execution_started_at = std::time::Instant::now();
                let response = if let Some(sequence) = active
                    .as_ref()
                    .and_then(|execution| execution.start_request_sequence)
                {
                    client.start_request_with_identity(
                        &request.execution_id,
                        sequence,
                        start_payload.clone(),
                        grant_use,
                    )?
                } else {
                    client.request(
                        EnvelopeKindV1::Start,
                        &request.execution_id,
                        start_payload.clone(),
                    )?
                };
                if !replaying_start
                    && let TargetLocation::Remote { worker_id } = &backend.target.location
                {
                    let _ =
                        crate::persistence::remote_execution_telemetry::record_execution_started(
                            &request.execution_id,
                            &request.task_id,
                            worker_id,
                            &request.image_digest,
                            execution_started_at
                                .elapsed()
                                .as_millis()
                                .min(u64::MAX as u128) as u64,
                        );
                }
                let upload = backend
                    .pending_workspace_transfers
                    .lock()
                    .ok()
                    .and_then(|mut pending| pending.remove(&lease.lease_id));
                if !replaying_start
                    && let (Some((bytes, ms)), TargetLocation::Remote { worker_id }) =
                        (upload, &backend.target.location)
                {
                    let _ = crate::persistence::remote_execution_telemetry::record_workspace_upload(
                        &request.execution_id,
                        &request.task_id,
                        worker_id,
                        bytes,
                        ms,
                    );
                }
                let mut handle: ExecutionHandleV1 =
                    serde_json::from_value(response.payload).map_err(|_| incompatible())?;
                if handle.execution_id != request.execution_id
                    || handle.task_id != request.task_id
                    || handle.request_sequence != request.request_sequence
                {
                    return Err(incompatible());
                }
                handle.target = backend.target.clone();
                (handle, replaying_start, execution_started_at, false)
            }
        };
        let (handle, replaying_start, execution_started_at, revoked_before_start) = start_result;
        backend.stop_if_private_grant_revoked(&handle).await?;
        if revoked_before_start {
            return Err(private_files::revoked_error());
        }
        backend
            .execution_snapshots
            .lock()
            .map_err(|_| disconnected())?
            .insert(handle.execution_id.clone(), snapshot);
        if !replaying_start {
            backend
                .execution_started_at
                .lock()
                .map_err(|_| disconnected())?
                .insert(handle.execution_id.clone(), execution_started_at);
        }
        Ok(handle)
    })
}

fn remove_unstarted_grant_link(
    backend: &RemoteExecutionBackend,
    execution_id: &str,
) -> Result<(), crate::execution::errors::ExecutionError> {
    let worker_id = match &backend.target.location {
        TargetLocation::Remote { worker_id } => worker_id,
        TargetLocation::Local => return Err(incompatible()),
    };
    crate::persistence::remote_private_file_executions::remove(
        &crate::persistence::state_root(),
        worker_id,
        execution_id,
    )
    .map_err(|_| disconnected())?;
    backend
        .execution_private_file_grants
        .lock()
        .map_err(|_| disconnected())?
        .remove(execution_id);
    Ok(())
}

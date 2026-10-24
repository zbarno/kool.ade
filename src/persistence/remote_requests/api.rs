use super::*;

pub fn save_pending(
    worker_id: &str,
    envelope: &crate::execution::remote::protocol::EnvelopeV1,
) -> anyhow::Result<()> {
    save_pending_at(&crate::persistence::state_root(), worker_id, envelope)
}

pub(super) fn save_pending_at(
    root: &Path,
    worker_id: &str,
    envelope: &crate::execution::remote::protocol::EnvelopeV1,
) -> anyhow::Result<()> {
    anyhow::ensure!(
        envelope.kind != crate::execution::remote::protocol::EnvelopeKindV1::PrivateFileChunk,
        "secret-bearing private file chunks cannot be persisted for retry"
    );
    envelope.validate()?;
    let directory = worker_directory(root, worker_id, true)?;
    let existing = pending_path(&directory, &envelope.request_id);
    let pending = PendingRemoteRequestV1 {
        schema_version: 1,
        worker_id: worker_id.to_owned(),
        created_at_unix_ms: crate::persistence::harness_settings::current_unix_ms(),
        envelope: envelope.clone(),
    };
    let bytes = serde_json::to_vec(&pending)?;
    match crate::artifacts::atomic_create_bytes(&existing, &bytes) {
        Ok(()) => {
            set_private_file(&existing)?;
            Ok(())
        }
        Err(_) => {
            let prior = read_pending(&existing)?;
            anyhow::ensure!(
                prior.worker_id == worker_id && prior.envelope == *envelope,
                "remote request ID is already bound to different pending work"
            );
            Ok(())
        }
    }
}

pub fn pending(worker_id: &str) -> anyhow::Result<Vec<PendingRemoteRequestV1>> {
    pending_at(&crate::persistence::state_root(), worker_id)
}

pub(super) fn pending_at(
    root: &Path,
    worker_id: &str,
) -> anyhow::Result<Vec<PendingRemoteRequestV1>> {
    let directory = worker_directory(root, worker_id, false)?;
    if !directory.exists() {
        return Ok(Vec::new());
    }
    let now = crate::persistence::harness_settings::current_unix_ms();
    let mut active = Vec::new();
    for request in load_pending_dir(&directory, worker_id)? {
        if now.saturating_sub(request.created_at_unix_ms) > super::REPLAY_RETENTION_MS {
            let error = crate::execution::errors::ExecutionError::new(
                crate::execution::errors::ExecutionErrorKind::WorkerIncompatible,
                "Remote request replay retention expired before the worker replied",
                false,
            );
            let response = crate::execution::remote::protocol::EnvelopeV1 {
                version: request.envelope.version,
                request_id: request.envelope.request_id.clone(),
                execution_id: request.envelope.execution_id.clone(),
                sequence: request.envelope.sequence,
                kind: crate::execution::remote::protocol::EnvelopeKindV1::Error,
                payload: serde_json::to_value(error)?,
            };
            record_completion_at(root, worker_id, &request.envelope.request_id, &response)?;
        } else {
            active.push(request);
        }
    }
    Ok(active)
}

pub fn record_completion(
    worker_id: &str,
    request_id: &str,
    response: &crate::execution::remote::protocol::EnvelopeV1,
) -> anyhow::Result<()> {
    record_completion_at(
        &crate::persistence::state_root(),
        worker_id,
        request_id,
        response,
    )
}

pub(super) fn record_completion_at(
    root: &Path,
    worker_id: &str,
    request_id: &str,
    response: &crate::execution::remote::protocol::EnvelopeV1,
) -> anyhow::Result<()> {
    anyhow::ensure!(
        response.kind != crate::execution::remote::protocol::EnvelopeKindV1::PrivateFileChunk,
        "secret-bearing private file chunks cannot be persisted as completions"
    );
    response.validate()?;
    anyhow::ensure!(
        response.request_id == request_id,
        "remote response request ID mismatch"
    );
    let directory = worker_directory(root, worker_id, true)?;
    let completed = CompletedRemoteRequestV1 {
        schema_version: 1,
        worker_id: worker_id.to_owned(),
        request_id: request_id.to_owned(),
        completed_at_unix_ms: crate::persistence::harness_settings::current_unix_ms(),
        response: response.clone(),
    };
    let path = completed_path(&directory, request_id);
    let bytes = serde_json::to_vec(&completed)?;
    match crate::artifacts::atomic_create_bytes(&path, &bytes) {
        Ok(()) => set_private_file(&path)?,
        Err(_) => {
            let prior = read_completed(&path)?;
            anyhow::ensure!(
                prior.response == *response,
                "remote request completion conflicts"
            );
        }
    }
    let pending = pending_path(&directory, request_id);
    match fs::remove_file(pending) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error.into()),
    }
    Ok(())
}

/// Retrieve and remove a completion reconciled after a controller restart.
pub fn take_completion(
    worker_id: &str,
    request_id: &str,
) -> anyhow::Result<Option<CompletedRemoteRequestV1>> {
    take_completion_at(&crate::persistence::state_root(), worker_id, request_id)
}

/// Retire a grant-bound Start request after revocation cleanup has proved the
/// worker stage is not mounted. A later generic reconnect must not dispatch it.
pub fn deny_pending_start(worker_id: &str, execution_id: &str) -> anyhow::Result<()> {
    deny_pending_start_at(&crate::persistence::state_root(), worker_id, execution_id)
}

pub(super) fn deny_pending_start_at(
    root: &Path,
    worker_id: &str,
    execution_id: &str,
) -> anyhow::Result<()> {
    let Some(request) = pending_at(root, worker_id)?.into_iter().find(|request| {
        request.envelope.request_id == execution_id
            && request.envelope.execution_id == execution_id
            && request.envelope.kind == crate::execution::remote::protocol::EnvelopeKindV1::Start
    }) else {
        return Ok(());
    };
    if let Some(completion) = take_completion_at(root, worker_id, execution_id)? {
        return record_completion_at(root, worker_id, execution_id, &completion.response);
    }
    let error = crate::execution::errors::ExecutionError::new(
        crate::execution::errors::ExecutionErrorKind::PolicyDenied,
        "Remote private-file approval was revoked before the worker accepted Start",
        false,
    );
    let response = crate::execution::remote::protocol::EnvelopeV1 {
        version: request.envelope.version,
        request_id: execution_id.to_owned(),
        execution_id: execution_id.to_owned(),
        sequence: request.envelope.sequence,
        kind: crate::execution::remote::protocol::EnvelopeKindV1::Error,
        payload: serde_json::to_value(error)?,
    };
    record_completion_at(root, worker_id, execution_id, &response)
}

pub(super) fn take_completion_at(
    root: &Path,
    worker_id: &str,
    request_id: &str,
) -> anyhow::Result<Option<CompletedRemoteRequestV1>> {
    anyhow::ensure!(valid_request_id(request_id), "remote request ID is invalid");
    let directory = worker_directory(root, worker_id, false)?;
    let path = completed_path(&directory, request_id);
    let completion = match read_completed(&path) {
        Ok(completion) => completion,
        Err(error)
            if error
                .downcast_ref::<std::io::Error>()
                .is_some_and(|io| io.kind() == std::io::ErrorKind::NotFound) =>
        {
            return Ok(None);
        }
        Err(error) => return Err(error),
    };
    anyhow::ensure!(
        completion.worker_id == worker_id,
        "remote completion worker mismatch"
    );
    fs::remove_file(path)?;
    Ok(Some(completion))
}

use super::*;
use std::time::Duration;

pub(super) fn dispatch(
    stager: Option<&Arc<crate::execution::remote::staging::WorkerStager>>,
    runtime: &tokio::runtime::Runtime,
    envelope: &EnvelopeV1,
) -> Result<(Kind, Value), ExecutionError> {
    match envelope.kind {
        Kind::AuthorizePrivateFileGrant => {
            let request: AuthorizePrivateFileGrantRequest = decode(&envelope.payload)?;
            require_execution_id(envelope, &request.execution_id)?;
            let stager = stager.ok_or_else(|| {
                super::super::incompatible("worker private-file staging is unavailable")
            })?;
            stager
                .authorize_private_file_grant(
                    &request.execution_id,
                    &request.workspace,
                    request.grant,
                    crate::persistence::harness_settings::current_unix_ms(),
                )
                .map_err(|_| {
                    super::super::incompatible("private-file grant authorization was rejected")
                })?;
            if let Ok(Some(expiry)) = stager.private_file_expiry(&request.execution_id) {
                expire_unstarted(stager.clone(), request.execution_id, expiry, runtime);
            }
            Ok((Kind::Acknowledge, json!({"authorized": true})))
        }
        Kind::AbortPrivateFileStage => {
            let request: AbortPrivateFileStageRequest = decode(&envelope.payload)?;
            require_execution_id(envelope, &request.execution_id)?;
            stager
                .ok_or_else(|| {
                    super::super::incompatible("worker private-file staging is unavailable")
                })?
                .cleanup_private_file_execution(&request.execution_id)
                .map_err(|_| {
                    super::super::incompatible("active private-file stage cannot be aborted")
                })?;
            Ok((Kind::Acknowledge, json!({"aborted": true})))
        }
        _ => unreachable!(),
    }
}

pub(super) fn expire_unstarted(
    stager: Arc<crate::execution::remote::staging::WorkerStager>,
    execution_id: String,
    expiry: u64,
    runtime: &tokio::runtime::Runtime,
) {
    let delay = expiry.saturating_sub(crate::persistence::harness_settings::current_unix_ms());
    runtime.spawn(async move {
        tokio::time::sleep(Duration::from_millis(delay)).await;
        loop {
            if stager.cleanup_private_file_execution(&execution_id).is_ok() {
                break;
            }
            tokio::time::sleep(Duration::from_secs(5)).await;
        }
    });
}

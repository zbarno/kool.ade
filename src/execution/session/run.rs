use crate::execution::{
    backend::ExecutionHandleV1,
    contract::ExecutionRequestV1,
    errors::{ExecutionError, ExecutionErrorKind},
};

use super::{ExecutionOutput, ExecutionSession};

mod attempt;
mod collect;
use attempt::confirmed_worker_loss;

impl ExecutionSession {
    pub(super) async fn run_request(
        &self,
        request: ExecutionRequestV1,
    ) -> Result<ExecutionOutput, ExecutionError> {
        self.run_request_inner(request, false)
            .await
            .map(|(output, _)| output)
    }

    pub(super) async fn run_persistent_request(
        &self,
        request: ExecutionRequestV1,
    ) -> Result<
        (
            ExecutionOutput,
            Option<(
                ExecutionHandleV1,
                crate::execution::backend::WorkspaceLeaseV1,
            )>,
        ),
        ExecutionError,
    > {
        self.run_request_inner(request, true).await
    }

    async fn run_request_inner(
        &self,
        request: ExecutionRequestV1,
        retain_on_success: bool,
    ) -> Result<
        (
            ExecutionOutput,
            Option<(
                ExecutionHandleV1,
                crate::execution::backend::WorkspaceLeaseV1,
            )>,
        ),
        ExecutionError,
    > {
        self.verify_authorization()?;
        let prepare = self
            .backend
            .prepare_workspace(&request.workspace, &request.target);
        let remaining = remaining(request.deadline_unix_ms)?;
        let lease = tokio::time::timeout(remaining, prepare)
            .await
            .map_err(|_| deadline_error())??;
        if lease.workspace != request.workspace || lease.expires_unix_ms <= now_unix_ms() {
            let _ = self.backend.release_workspace(&lease).await;
            return Err(internal_error());
        }
        if let Err(error) = self.verify_authorization() {
            let _ = self.backend.release_workspace(&lease).await;
            return Err(error);
        }
        if let Err(error) = self.ensure_not_cancelled() {
            let _ = self.backend.release_workspace(&lease).await;
            return Err(error);
        }
        if let Err(error) = self.refresh_and_prepare_remote_capacity().await {
            let _ = self.backend.release_workspace(&lease).await;
            return Err(error);
        }
        if let Err(error) = self.verify_authorization() {
            let _ = self.backend.release_workspace(&lease).await;
            return Err(error);
        }
        if matches!(
            &self.target.location,
            crate::execution::contract::TargetLocation::Remote { .. }
        ) && let Some(verifier) = &self.authorization_verifier
            && let Err(error) = verifier.mark_remote_attempt_started()
        {
            let _ = self.backend.release_workspace(&lease).await;
            return Err(error);
        }
        let handle = match self.backend.start(&lease, &request).await {
            Ok(handle) => handle,
            Err(error) => {
                if error.kind == ExecutionErrorKind::WorkerIncompatible
                    && error.message
                        == crate::execution::errors::REMOTE_START_CAPACITY_REJECTION_MESSAGE
                {
                    let _ = self.rollback_rejected_remote_capacity_start();
                    if let Some(verifier) = &self.authorization_verifier
                        && let Err(fence_error) = verifier.finish_remote_attempt()
                    {
                        let _ = self.backend.release_workspace(&lease).await;
                        return Err(fence_error);
                    }
                }
                let _ = self.backend.release_workspace(&lease).await;
                return Err(error);
            }
        };
        if handle.execution_id != request.execution_id
            || handle.target != request.target
            || handle.request_sequence != request.request_sequence
        {
            let _ = self.backend.release(&handle).await;
            let _ = self.backend.release_workspace(&lease).await;
            return Err(internal_error());
        }
        if let Err(error) = self.mark_remote_capacity_running() {
            let _ = self
                .backend
                .cancel(&handle, request.cancellation_generation)
                .await;
            let _ = self.backend.release(&handle).await;
            let _ = self.backend.release_workspace(&lease).await;
            return Err(error);
        }
        if let Err(error) = self.verify_authorization() {
            let _ = self
                .backend
                .cancel(&handle, request.cancellation_generation)
                .await;
            let _ = self.backend.release(&handle).await;
            let _ = self.backend.release_workspace(&lease).await;
            return Err(error);
        }
        let generation = request.cancellation_generation;
        if let Ok(mut active) = self.active.lock() {
            *active = Some((handle.clone(), generation));
        } else {
            let _ = self.backend.cancel(&handle, generation).await;
            let _ = self.backend.release(&handle).await;
            let _ = self.backend.release_workspace(&lease).await;
            return Err(internal_error());
        }
        if self.ensure_not_cancelled().is_err() {
            let _ = self.backend.cancel(&handle, generation).await;
            let _ = self.backend.release(&handle).await;
            let _ = self.backend.release_workspace(&lease).await;
            return Err(cancelled_error());
        }
        let mut result = self.collect_request(&handle, &request).await;
        if let (Ok(output), crate::execution::contract::TargetLocation::Remote { .. }) =
            (&result, &self.target.location)
            && let Some(verifier) = &self.authorization_verifier
        {
            let transition = if confirmed_worker_loss(&output.result) {
                let active = self.active.lock().map_err(|_| internal_error())?.clone();
                if let Some((handle, _)) = active {
                    self.backend.discard_interrupted_result(&handle).await?;
                }
                verifier.confirm_remote_attempt_lost()
            } else {
                verifier.finish_remote_attempt()
            };
            if let Err(error) = transition {
                result = Err(error);
            }
        }
        if result.is_err()
            || self
                .cancel_requested
                .load(std::sync::atomic::Ordering::Acquire)
        {
            let _ = self.backend.cancel(&handle, generation).await;
        }
        let retain = result.as_ref().is_ok_and(|output| {
            retain_on_success
                && output.result.exit_code == Some(0)
                && !output.result.cancelled
                && !output.result.timed_out
        });
        if retain {
            return result.map(|output| (output, Some((handle, lease))));
        }
        let release = self.backend.release(&handle).await;
        let workspace_release = self.backend.release_workspace(&lease).await;
        match (result, release, workspace_release) {
            (Ok(output), _, _) if confirmed_worker_loss(&output.result) => Ok((output, None)),
            (Ok(output), Ok(()), Ok(())) => Ok((output, None)),
            (Err(error), _, _) => Err(error),
            (_, Err(error), _) | (_, _, Err(error)) => Err(error),
        }
    }

    async fn refresh_and_prepare_remote_capacity(&self) -> Result<(), ExecutionError> {
        let crate::execution::contract::TargetLocation::Remote { worker_id } =
            &self.target.location
        else {
            return Ok(());
        };
        if !self.has_remote_capacity_lease()? {
            return Ok(());
        }
        let probe = self.backend.probe(&self.target).await?;
        let capabilities = probe.capabilities.ok_or_else(|| {
            ExecutionError::new(
                ExecutionErrorKind::WorkerIncompatible,
                "Remote worker did not report execution capacity",
                false,
            )
        })?;
        let registration = crate::persistence::remote_workers::get(worker_id)
            .map_err(|_| internal_error())?
            .ok_or_else(|| {
                ExecutionError::new(
                    ExecutionErrorKind::WorkerIncompatible,
                    "Remote worker enrollment is unavailable",
                    false,
                )
            })?;
        if !registration.enabled {
            return Err(ExecutionError::new(
                ExecutionErrorKind::WorkerIncompatible,
                "Remote worker is disabled",
                false,
            ));
        }
        let max_jobs = capabilities
            .maximum_execution_count
            .map_or(registration.max_jobs, |maximum| {
                maximum.min(registration.max_jobs)
            });
        self.prepare_remote_capacity_start(
            max_jobs,
            capabilities.current_execution_count,
            worker_id,
            &registration.host_key_fingerprint,
        )
    }
}

pub(super) fn append_bounded(
    output: &mut String,
    chunk: String,
    captured_bytes: &mut u64,
    limit: u64,
) -> Result<(), ExecutionError> {
    let chunk_bytes = u64::try_from(chunk.len()).unwrap_or(u64::MAX);
    if captured_bytes.saturating_add(chunk_bytes) > limit {
        return Err(ExecutionError::new(
            ExecutionErrorKind::OutputLimitExceeded,
            "Execution output exceeded the session limit",
            false,
        ));
    }
    *captured_bytes += chunk_bytes;
    output.push_str(&chunk);
    Ok(())
}

fn cancelled_error() -> ExecutionError {
    ExecutionError::new(
        ExecutionErrorKind::Cancelled,
        "Execution was cancelled before the command completed",
        false,
    )
}

fn internal_error() -> ExecutionError {
    ExecutionError::new(
        ExecutionErrorKind::WorkerDisconnected,
        "Execution backend returned an invalid lifecycle response",
        true,
    )
}

fn remaining(deadline_unix_ms: u64) -> Result<std::time::Duration, ExecutionError> {
    let remaining = deadline_unix_ms.saturating_sub(now_unix_ms());
    if remaining == 0 {
        Err(deadline_error())
    } else {
        Ok(std::time::Duration::from_millis(remaining))
    }
}

fn now_unix_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .min(u64::MAX as u128) as u64
}

fn deadline_error() -> ExecutionError {
    ExecutionError::new(
        ExecutionErrorKind::DeadlineExceeded,
        "Execution deadline elapsed while preparing the workspace",
        false,
    )
}

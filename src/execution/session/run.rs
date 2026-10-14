use crate::execution::{
    backend::ExecutionHandleV1,
    contract::ExecutionRequestV1,
    errors::{ExecutionError, ExecutionErrorKind},
};

use super::{ExecutionOutput, ExecutionSession};

mod collect;

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
        let handle = match self.backend.start(&lease, &request).await {
            Ok(handle) => handle,
            Err(error) => {
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
        let result = self.collect_request(&handle, &request).await;
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
            (Ok(output), Ok(()), Ok(())) => Ok((output, None)),
            (Err(error), _, _) => Err(error),
            (_, Err(error), _) | (_, _, Err(error)) => Err(error),
        }
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

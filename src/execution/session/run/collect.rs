use crate::execution::{
    backend::ExecutionHandleV1,
    contract::{
        EventAcceptance, EventPageRequestV1, EventStreamValidator, ExecutionEventKindV1,
        ExecutionLifecycleV1, ExecutionRequestV1,
    },
    errors::ExecutionError,
};

use super::super::{ExecutionOutput, ExecutionSession};
use super::{append_bounded, internal_error, remaining};

impl ExecutionSession {
    pub(super) async fn collect_request(
        &self,
        handle: &ExecutionHandleV1,
        request: &ExecutionRequestV1,
    ) -> Result<ExecutionOutput, ExecutionError> {
        self.verify_authorization()?;
        self.backend.exec(handle, request).await?;
        self.verify_authorization()?;
        let mut validator = EventStreamValidator::default();
        let mut after_sequence = 0;
        let mut stdout = String::new();
        let mut stderr = String::new();
        let mut captured_output_bytes = 0_u64;
        let mut terminal_state = None;
        loop {
            self.verify_authorization()?;
            self.ensure_not_cancelled()?;
            let wait = remaining(request.deadline_unix_ms)?;
            let events = self
                .backend
                .events(
                    handle,
                    EventPageRequestV1::new(after_sequence, 128).map_err(|_| internal_error())?,
                )
                .await?;
            if events.is_empty() {
                if terminal_state.is_some() {
                    break;
                }
                tokio::time::sleep(wait.min(std::time::Duration::from_millis(25))).await;
                continue;
            }
            for event in events {
                if event.execution_id != request.execution_id {
                    return Err(internal_error());
                }
                if validator.accept(&event).map_err(|_| internal_error())?
                    == EventAcceptance::Duplicate
                {
                    continue;
                }
                after_sequence = event.sequence;
                match event.event {
                    ExecutionEventKindV1::Stdout { chunk } => {
                        append_bounded(
                            &mut stdout,
                            chunk,
                            &mut captured_output_bytes,
                            request.resource_budget.output_bytes,
                        )?;
                    }
                    ExecutionEventKindV1::Stderr { chunk } => {
                        append_bounded(
                            &mut stderr,
                            chunk,
                            &mut captured_output_bytes,
                            request.resource_budget.output_bytes,
                        )?;
                    }
                    ExecutionEventKindV1::Lifecycle { state }
                        if matches!(
                            state,
                            ExecutionLifecycleV1::Succeeded
                                | ExecutionLifecycleV1::Failed
                                | ExecutionLifecycleV1::Cancelled
                                | ExecutionLifecycleV1::Interrupted
                        ) =>
                    {
                        terminal_state = Some(state)
                    }
                    _ => {}
                }
            }
        }
        // The app-owned authorization generation can change while the worker
        // runs. Fence result collection before accepting any worker output.
        self.verify_authorization()?;
        let result = self.backend.collect_artifacts(handle).await?;
        if result.execution_id != request.execution_id {
            return Err(internal_error());
        }
        result.validate().map_err(|_| internal_error())?;
        let outcome_matches = match terminal_state {
            Some(ExecutionLifecycleV1::Succeeded) => {
                result.exit_code == Some(0)
                    && !result.cancelled
                    && !result.timed_out
                    && result.failure.is_none()
            }
            Some(ExecutionLifecycleV1::Cancelled) => {
                result.cancelled && result.exit_code.is_none() && result.signal.is_none()
            }
            Some(ExecutionLifecycleV1::Failed) => {
                !result.cancelled && (result.failure.is_some() || result.exit_code != Some(0))
            }
            Some(ExecutionLifecycleV1::Interrupted) => {
                result.failure.is_some()
                    && result.exit_code.is_none()
                    && result.signal.is_none()
                    && !result.cancelled
                    && !result.timed_out
                    && result.failure.as_ref().is_some_and(|failure| {
                        failure.kind
                            == crate::execution::errors::ExecutionErrorKind::WorkerDisconnected
                            && failure.retryable
                    })
            }
            _ => false,
        };
        if !outcome_matches {
            return Err(internal_error());
        }
        // The result transaction runs only while the app-owned claim/capability
        // generation remains the one captured by this request.
        self.verify_authorization()?;
        if terminal_state != Some(ExecutionLifecycleV1::Interrupted) {
            self.backend.apply_workspace_result(handle, request).await?;
            self.verify_authorization()?;
        }
        Ok(ExecutionOutput {
            stdout,
            stderr,
            result,
        })
    }
}

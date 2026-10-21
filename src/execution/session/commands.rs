use std::{collections::BTreeSet, sync::atomic::Ordering, time::Duration};

use crate::execution::errors::ExecutionError;

mod qa;
mod recipe;
mod retry;

use super::{ExecutionOutput, ExecutionSession, RequestParameters, policy_error};

impl ExecutionSession {
    /// Executes a bounded shell script on stdin inside the selected execution
    /// backend. The shell and script are never interpolated into a host command.
    pub fn run_bounded_script(
        &self,
        script: &str,
        timeout: Duration,
    ) -> Result<ExecutionOutput, ExecutionError> {
        if let Some(destination) = self.retry_destination()? {
            return destination.run_bounded_script(script, timeout);
        }
        self.verify_authorization()?;
        self.renew_remote_capacity_lease()?;
        if script.len() > crate::execution::contract::MAX_SCRIPT_BYTES
            || script.contains('\0')
            || timeout.is_zero()
            || timeout > Duration::from_secs(1200)
        {
            return Err(policy_error(
                "Shell script or execution timeout is outside limits",
            ));
        }
        if self
            .busy
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
        {
            return Err(policy_error(
                "An execution is already active in this session",
            ));
        }
        self.cancel_requested.store(false, Ordering::Release);
        let sequence = self.sequence.fetch_add(1, Ordering::Relaxed);
        let cancellation_generation = self.cancellation_generation.fetch_add(1, Ordering::AcqRel);
        let request = match self.request(RequestParameters {
            argv: vec!["sh".into(), "-s".into()],
            stdin: Some(script.to_owned()),
            working_directory_relative: ".".into(),
            environment_allowlist: BTreeSet::new(),
            timeout,
            request_sequence: sequence,
            cancellation_generation,
            policy_override: None,
            artifact_paths: Vec::new(),
        }) {
            Ok(request) => request,
            Err(error) => {
                self.busy.store(false, Ordering::Release);
                return Err(error);
            }
        };
        let result = self.runtime.block_on(self.run_request(request));
        self.clear_active();
        let confirmed_loss = result
            .as_ref()
            .is_ok_and(|output| super::is_confirmed_remote_worker_loss(&output.result));
        if result.is_ok() && !confirmed_loss {
            self.verify_authorization()?;
        }
        self.retry_after_confirmed_loss_with(result, |destination| {
            destination.run_bounded_script(script, timeout)
        })
    }
}

#[cfg(test)]
#[path = "commands/tests.rs"]
mod tests;

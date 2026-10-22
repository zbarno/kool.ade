use super::super::{ExecutionOutput, ExecutionSession, policy_error};
use crate::execution::errors::ExecutionError;
use std::sync::Arc;

impl ExecutionSession {
    pub(super) fn retry_after_confirmed_loss_with(
        &self,
        result: Result<ExecutionOutput, ExecutionError>,
        retry: impl FnOnce(Arc<ExecutionSession>) -> Result<ExecutionOutput, ExecutionError>,
    ) -> Result<ExecutionOutput, ExecutionError> {
        let confirmed_loss = result
            .as_ref()
            .is_ok_and(|output| super::super::is_confirmed_remote_worker_loss(&output.result));
        if !confirmed_loss {
            return result;
        }
        self.release_remote_capacity_lease()?;
        let factory = self
            .remote_retry_factory
            .lock()
            .map_err(|_| policy_error("Remote retry state is unavailable"))?
            .clone();
        let Some(factory) = factory else {
            return result;
        };
        let Ok(destination) = factory() else {
            return result;
        };
        self.copy_telemetry_attribution_to(&destination);
        self.install_retry_destination(Arc::clone(&destination))?;
        retry(destination)
    }
}

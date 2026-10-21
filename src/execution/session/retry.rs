use super::*;

pub(crate) fn is_confirmed_remote_worker_loss(result: &ExecutionResultV1) -> bool {
    result.exit_code.is_none()
        && result.signal.is_none()
        && !result.cancelled
        && !result.timed_out
        && result.failure.as_ref().is_some_and(|failure| {
            failure.kind == ExecutionErrorKind::WorkerDisconnected && failure.retryable
        })
}

impl ExecutionSession {
    pub(crate) fn set_remote_retry_factory(
        &self,
        factory: Arc<RemoteRetryFactory>,
    ) -> Result<(), ExecutionError> {
        *self
            .remote_retry_factory
            .lock()
            .map_err(|_| internal_error())? = Some(factory);
        Ok(())
    }

    pub(crate) fn retry_destination(
        &self,
    ) -> Result<Option<Arc<ExecutionSession>>, ExecutionError> {
        self.retry_destination
            .lock()
            .map(|destination| destination.clone())
            .map_err(|_| internal_error())
    }

    pub(super) fn install_retry_destination(
        &self,
        destination: Arc<ExecutionSession>,
    ) -> Result<(), ExecutionError> {
        *self
            .retry_destination
            .lock()
            .map_err(|_| internal_error())? = Some(destination);
        Ok(())
    }
}

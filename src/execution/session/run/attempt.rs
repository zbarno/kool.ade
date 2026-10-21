pub(super) fn confirmed_worker_loss(
    result: &crate::execution::contract::ExecutionResultV1,
) -> bool {
    result.exit_code.is_none()
        && result.signal.is_none()
        && !result.cancelled
        && !result.timed_out
        && result.failure.as_ref().is_some_and(|failure| {
            failure.kind == crate::execution::errors::ExecutionErrorKind::WorkerDisconnected
                && failure.retryable
        })
}

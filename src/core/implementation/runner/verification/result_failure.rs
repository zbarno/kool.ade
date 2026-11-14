pub(super) fn classify_execution_error(
    error: crate::execution::errors::ExecutionError,
) -> anyhow::Error {
    if is_uncertain_disconnect(&error) {
        anyhow::Error::new(super::super::VerificationOutcomeUnknown)
    } else {
        anyhow::anyhow!("Docker verification failed: {}", error.message)
    }
}

pub(super) fn classify_result_failure(
    failure: Option<&crate::execution::errors::ExecutionError>,
) -> Option<anyhow::Error> {
    failure
        .filter(|error| is_uncertain_disconnect(error))
        .cloned()
        .map(classify_execution_error)
}

fn is_uncertain_disconnect(error: &crate::execution::errors::ExecutionError) -> bool {
    error.is_local_docker_health_disconnect()
        // A health probe can cross a backend adapter that changes the
        // ExecutionErrorKind. The exact canonical message still means the
        // command's completion is unknown, so never dispatch a correction.
        || error.message == crate::execution::errors::LOCAL_DOCKER_HEALTH_DISCONNECTED
        || error.kind == crate::execution::errors::ExecutionErrorKind::WorkerDisconnected
}

#[cfg(test)]
mod tests {
    use super::super::super::VerificationOutcomeUnknown;
    use super::{classify_execution_error, classify_result_failure};

    #[test]
    fn docker_health_disconnect_after_command_start_is_typed_as_unknown() {
        let error = crate::execution::errors::ExecutionError::new(
            crate::execution::errors::ExecutionErrorKind::EnvironmentUnavailable,
            crate::execution::errors::LOCAL_DOCKER_HEALTH_DISCONNECTED,
            true,
        );
        let classified = classify_execution_error(error);

        assert!(
            classified
                .downcast_ref::<VerificationOutcomeUnknown>()
                .is_some()
        );
        assert!(classified.to_string().contains("outcome is unknown"));
    }

    #[test]
    fn completed_result_health_disconnect_remains_typed_as_unknown() {
        let failure = crate::execution::errors::ExecutionError::new(
            crate::execution::errors::ExecutionErrorKind::EnvironmentUnavailable,
            crate::execution::errors::LOCAL_DOCKER_HEALTH_DISCONNECTED,
            false,
        );
        let classified = classify_result_failure(Some(&failure)).unwrap();

        assert!(
            classified
                .downcast_ref::<VerificationOutcomeUnknown>()
                .is_some()
        );
        assert!(classified.to_string().contains("outcome is unknown"));
    }

    #[test]
    fn completed_worker_disconnect_health_failure_remains_typed_as_unknown() {
        let failure = crate::execution::errors::ExecutionError::new(
            crate::execution::errors::ExecutionErrorKind::WorkerDisconnected,
            crate::execution::errors::LOCAL_DOCKER_HEALTH_DISCONNECTED,
            true,
        );
        let classified = classify_result_failure(Some(&failure)).unwrap();

        assert!(
            classified
                .downcast_ref::<VerificationOutcomeUnknown>()
                .is_some()
        );
        assert!(classified.to_string().contains("outcome is unknown"));
    }

    #[test]
    fn health_disconnect_with_adapter_error_kind_remains_typed_as_unknown() {
        let error = crate::execution::errors::ExecutionError::new(
            crate::execution::errors::ExecutionErrorKind::ExecutionFailed,
            crate::execution::errors::LOCAL_DOCKER_HEALTH_DISCONNECTED,
            false,
        );
        let classified = classify_execution_error(error);

        assert!(
            classified
                .downcast_ref::<VerificationOutcomeUnknown>()
                .is_some()
        );
        assert!(classified.to_string().contains("outcome is unknown"));
    }

    #[test]
    fn completed_health_disconnect_with_adapter_error_kind_is_unknown() {
        let failure = crate::execution::errors::ExecutionError::new(
            crate::execution::errors::ExecutionErrorKind::ExecutionFailed,
            crate::execution::errors::LOCAL_DOCKER_HEALTH_DISCONNECTED,
            false,
        );
        let classified = classify_result_failure(Some(&failure)).unwrap();

        assert!(
            classified
                .downcast_ref::<VerificationOutcomeUnknown>()
                .is_some()
        );
        assert!(classified.to_string().contains("outcome is unknown"));
    }

    #[test]
    fn any_completed_worker_disconnect_is_typed_as_unknown() {
        let failure = crate::execution::errors::ExecutionError::new(
            crate::execution::errors::ExecutionErrorKind::WorkerDisconnected,
            "Docker Engine operation failed or disconnected",
            true,
        );
        let classified = classify_result_failure(Some(&failure)).unwrap();

        assert!(
            classified
                .downcast_ref::<VerificationOutcomeUnknown>()
                .is_some()
        );
        assert!(classified.to_string().contains("outcome is unknown"));
    }

    #[test]
    fn non_health_errors_remain_ordinary_verification_failures() {
        let error = crate::execution::errors::ExecutionError::new(
            crate::execution::errors::ExecutionErrorKind::ExecutionFailed,
            "test command exited with status 1",
            false,
        );
        let classified = classify_execution_error(error);

        assert!(
            classified
                .downcast_ref::<VerificationOutcomeUnknown>()
                .is_none()
        );
        assert!(
            classified
                .to_string()
                .contains("Docker verification failed")
        );
    }
}

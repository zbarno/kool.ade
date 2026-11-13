pub(super) fn classify_execution_error(
    error: crate::execution::errors::ExecutionError,
) -> anyhow::Error {
    if error.is_local_docker_health_disconnect() {
        anyhow::Error::new(super::super::VerificationOutcomeUnknown)
    } else {
        anyhow::anyhow!("Docker verification failed: {}", error.message)
    }
}

pub(super) fn classify_result_failure(
    failure: Option<&crate::execution::errors::ExecutionError>,
) -> Option<anyhow::Error> {
    failure
        .filter(|error| error.is_local_docker_health_disconnect())
        .cloned()
        .map(classify_execution_error)
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
        for kind in [
            crate::execution::errors::ExecutionErrorKind::EnvironmentUnavailable,
            crate::execution::errors::ExecutionErrorKind::WorkerDisconnected,
            crate::execution::errors::ExecutionErrorKind::ExecutionFailed,
        ] {
            let failure = crate::execution::errors::ExecutionError::new(
                kind,
                crate::execution::errors::LOCAL_DOCKER_HEALTH_DISCONNECTED,
                false,
            );
            let classified = classify_result_failure(Some(&failure)).unwrap();

            assert!(
                classified
                    .downcast_ref::<VerificationOutcomeUnknown>()
                    .is_some(),
                "exact local Docker health failure reported as {kind:?} must stay uncertain"
            );
            assert!(classified.to_string().contains("outcome is unknown"));
        }
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

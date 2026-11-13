use serde::{Deserialize, Serialize};

pub const MAX_EXECUTION_ERROR_MESSAGE_BYTES: usize = 4096;
pub const REMOTE_START_CAPACITY_REJECTION: &str = "remote worker execution capacity is full";
pub const REMOTE_START_CAPACITY_REJECTION_MESSAGE: &str =
    "Remote worker Start request failed: remote worker execution capacity is full";
pub const LOCAL_START_CAPACITY_REJECTION: &str = "Local worker CPU or memory reservation is full";
pub const LOCAL_CAPACITY_UNKNOWN_REJECTION: &str = "Cannot start a local execution while CPU or memory capacity is unknown and another execution owns host capacity";
pub const LOCAL_DOCKER_HEALTH_DISCONNECTED: &str =
    "Docker Engine disconnected or did not answer its health request";

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ExecutionErrorKind {
    EnvironmentUnavailable,
    PolicyDenied,
    DependencyRequired,
    ImageUnavailable,
    WorkerDisconnected,
    WorkerIncompatible,
    ExecutionFailed,
    Cancelled,
    DeadlineExceeded,
    EventSequenceInvalid,
    OutputLimitExceeded,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ExecutionError {
    pub kind: ExecutionErrorKind,
    pub message: String,
    pub retryable: bool,
}

impl ExecutionError {
    pub fn new(kind: ExecutionErrorKind, message: impl Into<String>, retryable: bool) -> Self {
        Self {
            kind,
            message: message.into(),
            retryable,
        }
    }

    pub fn validate(&self) -> anyhow::Result<()> {
        anyhow::ensure!(
            !self.message.is_empty()
                && self.message.len() <= MAX_EXECUTION_ERROR_MESSAGE_BYTES
                && !self.message.chars().any(char::is_control),
            "execution failure message is invalid or too large"
        );
        Ok(())
    }

    pub fn is_retryable_local_capacity_rejection(&self) -> bool {
        self.kind == ExecutionErrorKind::EnvironmentUnavailable
            && self.retryable
            && matches!(
                self.message.as_str(),
                LOCAL_START_CAPACITY_REJECTION | LOCAL_CAPACITY_UNKNOWN_REJECTION
            )
    }

    /// Identifies the exact local Docker engine health-disconnect failure.
    /// This describes the error only; callers must decide whether retrying
    /// is safe based on whether user work may already have started.
    pub fn is_local_docker_health_disconnect(&self) -> bool {
        matches!(
            self.kind,
            ExecutionErrorKind::EnvironmentUnavailable | ExecutionErrorKind::WorkerDisconnected
        ) && self.message == LOCAL_DOCKER_HEALTH_DISCONNECTED
    }

    /// A retryable health disconnect from the readiness probe before an
    /// execution session or command was created. Retrying here cannot replay
    /// user work.
    pub fn is_retryable_local_docker_health_disconnect(&self) -> bool {
        self.retryable && self.is_local_docker_health_disconnect()
    }
}

impl std::fmt::Display for ExecutionError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{:?}: {}", self.kind, self.message)
    }
}

impl std::error::Error for ExecutionError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn local_capacity_rejection_is_retryable_only_for_the_typed_capacity_error() {
        assert!(
            ExecutionError::new(
                ExecutionErrorKind::EnvironmentUnavailable,
                LOCAL_START_CAPACITY_REJECTION,
                true,
            )
            .is_retryable_local_capacity_rejection()
        );
        assert!(
            ExecutionError::new(
                ExecutionErrorKind::EnvironmentUnavailable,
                LOCAL_CAPACITY_UNKNOWN_REJECTION,
                true,
            )
            .is_retryable_local_capacity_rejection()
        );
        assert!(
            !ExecutionError::new(
                ExecutionErrorKind::EnvironmentUnavailable,
                LOCAL_START_CAPACITY_REJECTION,
                false,
            )
            .is_retryable_local_capacity_rejection()
        );
        assert!(
            !ExecutionError::new(
                ExecutionErrorKind::EnvironmentUnavailable,
                "Docker daemon is unavailable",
                true,
            )
            .is_retryable_local_capacity_rejection()
        );
    }

    #[test]
    fn local_docker_health_disconnect_is_retryable_only_before_execution() {
        assert!(
            ExecutionError::new(
                ExecutionErrorKind::EnvironmentUnavailable,
                LOCAL_DOCKER_HEALTH_DISCONNECTED,
                true,
            )
            .is_retryable_local_docker_health_disconnect()
        );
        assert!(
            !ExecutionError::new(
                ExecutionErrorKind::EnvironmentUnavailable,
                LOCAL_DOCKER_HEALTH_DISCONNECTED,
                false,
            )
            .is_retryable_local_docker_health_disconnect()
        );
        assert!(
            !ExecutionError::new(
                ExecutionErrorKind::ExecutionFailed,
                LOCAL_DOCKER_HEALTH_DISCONNECTED,
                true,
            )
            .is_retryable_local_docker_health_disconnect()
        );
        assert!(
            ExecutionError::new(
                ExecutionErrorKind::WorkerDisconnected,
                LOCAL_DOCKER_HEALTH_DISCONNECTED,
                true,
            )
            .is_retryable_local_docker_health_disconnect()
        );
        assert!(
            !ExecutionError::new(
                ExecutionErrorKind::EnvironmentUnavailable,
                "Docker Engine is unavailable",
                true,
            )
            .is_retryable_local_docker_health_disconnect()
        );
    }
}

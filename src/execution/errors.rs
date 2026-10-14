use serde::{Deserialize, Serialize};

pub const MAX_EXECUTION_ERROR_MESSAGE_BYTES: usize = 4096;

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
}

impl std::fmt::Display for ExecutionError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{:?}: {}", self.kind, self.message)
    }
}

impl std::error::Error for ExecutionError {}

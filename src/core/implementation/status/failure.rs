use serde::{Deserialize, Serialize};
use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FailureKind {
    RemoteDiverged,
    NoImplementationChanges,
    ExternalPrerequisite,
    Harness,
    Verification,
    Git,
    Cancelled,
    InvalidSavedState,
    Other,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RecoveryDisposition {
    AutomaticRetry,
    ExplicitResume,
    UserAction,
    DoNotRetry,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Failure {
    pub kind: FailureKind,
    pub recovery: RecoveryDisposition,
    pub message: String,
}

impl fmt::Display for Failure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl Failure {
    pub fn new(
        kind: FailureKind,
        recovery: RecoveryDisposition,
        message: impl Into<String>,
    ) -> Self {
        Self {
            kind,
            recovery,
            message: message.into(),
        }
    }

    pub fn other(message: impl Into<String>) -> Self {
        Self::new(
            FailureKind::Other,
            RecoveryDisposition::ExplicitResume,
            message,
        )
    }

    /// Convert a v0 queue's prose-only blocker into a durable decision type.
    pub fn from_legacy(message: String) -> Self {
        let lower = message.to_ascii_lowercase();
        let (kind, recovery) = if lower.contains("diverged before publication") {
            (
                FailureKind::RemoteDiverged,
                RecoveryDisposition::AutomaticRetry,
            )
        } else if lower.contains("no implementation changes relative to the starting commit") {
            (
                FailureKind::NoImplementationChanges,
                RecoveryDisposition::ExplicitResume,
            )
        } else {
            (FailureKind::Other, RecoveryDisposition::UserAction)
        };
        Self::new(kind, recovery, message)
    }

    pub fn from_error(error: &anyhow::Error) -> Self {
        error
            .downcast_ref::<FailureCause>()
            .map(|cause| cause.0.clone())
            .unwrap_or_else(|| Self::other(format!("{error:#}")))
    }
}

#[derive(Debug)]
pub struct FailureCause(pub Failure);

impl fmt::Display for FailureCause {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0.message)
    }
}

impl std::error::Error for FailureCause {}

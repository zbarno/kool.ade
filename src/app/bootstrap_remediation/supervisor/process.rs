use crate::app::bootstrap_remediation::supervisor::Cancellation;

mod capture;
mod claude_code_install;
mod claude_download;
mod platform;
mod program;
mod runner;
mod terminate;

#[cfg(test)]
mod tests;

const OUTPUT_LIMIT: usize = 64 * 1024;
const READER_JOIN_TIMEOUT: std::time::Duration = std::time::Duration::from_millis(500);

pub(crate) use program::{
    PublisherSignatureVerifier, TrustedArgument, TrustedInvocation, TrustedProgram,
    VerifiedArtifact, verify_plan_artifact,
};

pub(crate) struct SystemCommandRunner;

pub(crate) trait NativeCommandRunner {
    fn run(
        &self,
        invocation: TrustedInvocation,
        cancellation: &Cancellation,
    ) -> Result<ProcessOutput, ProcessRunError>;

    fn run_elevated(
        &self,
        _invocation: TrustedInvocation,
        _elevation: super::ElevationReceipt,
        _cancellation: &Cancellation,
    ) -> Result<ProcessOutput, ProcessRunError> {
        Err(ProcessRunError::not_started(ProcessRunFailure::Unavailable))
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ProcessRunFailure {
    Unavailable,
    Cancelled,
    TimedOut,
    FailedBeforeLaunch,
    FailedAfterLaunch,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct ProcessRunError {
    pub(super) launched: bool,
    pub(super) failure: ProcessRunFailure,
}

impl ProcessRunError {
    pub(crate) fn not_started(failure: ProcessRunFailure) -> Self {
        Self {
            launched: false,
            failure,
        }
    }

    pub(crate) fn after_launch(failure: ProcessRunFailure) -> Self {
        Self {
            launched: true,
            failure,
        }
    }
}

impl std::fmt::Display for ProcessRunError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self.failure {
            ProcessRunFailure::Cancelled => formatter.write_str("remediation was cancelled"),
            ProcessRunFailure::TimedOut => formatter.write_str("remediation timed out"),
            ProcessRunFailure::Unavailable => {
                formatter.write_str("remediation runner is unavailable")
            }
            ProcessRunFailure::FailedBeforeLaunch => {
                formatter.write_str("remediation failed before launch")
            }
            ProcessRunFailure::FailedAfterLaunch => {
                formatter.write_str("remediation failed after launch")
            }
        }
    }
}

impl std::error::Error for ProcessRunError {}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ProcessOutput {
    pub(crate) exit_code: i32,
    pub(crate) stdout: String,
    pub(crate) stderr: String,
    pub(crate) stdout_truncated: bool,
    pub(crate) stderr_truncated: bool,
}

use std::path::Path;

use crate::core::implementation::{Failure, FailureKind, RecoveryDisposition};

use super::super::report::{self, BlockerDisposition, Report};

pub(super) fn error(report: &Report, report_path: &Path) -> anyhow::Error {
    let kind = if report.blocker_disposition == BlockerDisposition::EnvironmentPrerequisite {
        FailureKind::ExternalPrerequisite
    } else {
        FailureKind::RemoteDiverged
    };
    anyhow::Error::new(crate::core::implementation::status::FailureCause(
        Failure::new(
            kind,
            RecoveryDisposition::UserAction,
            report::external_blocker_detail(report, report_path),
        ),
    ))
}

pub(super) fn verification_error(error: &str, evidence_path: &Path) -> anyhow::Error {
    anyhow::Error::new(crate::core::implementation::status::FailureCause(
        Failure::new(
            FailureKind::ExternalPrerequisite,
            RecoveryDisposition::UserAction,
            format!(
                "## Waiting for environment\n\nRequired baseline verification cannot obtain a tool or dependency in the network-isolated sandbox. The merge worktree and both histories are preserved.\n\n### Evidence\n\n{error}\n\n### Next action(s)\n\n- Seed the required package cache or provide a controlled dependency source, then resume implementation.\n\nVerification log: {}",
                evidence_path.display()
            ),
        ),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn environment_blocker_uses_environment_failure_kind() {
        let report = report::parse_report(
            r#"{"schemaVersion":2,"status":"blocked","blocker_disposition":"environment_prerequisite","summary":"No package cache is available.","acceptance_criteria":[],"verification":[],"remaining":["Seed npm cache"]}"#,
        )
        .unwrap();
        let failure = Failure::from_error(&error(&report, Path::new("report.json")));
        assert_eq!(failure.kind, FailureKind::ExternalPrerequisite);
        assert!(failure.message.contains("Waiting for environment"));
    }

    #[test]
    fn failed_verification_environment_issue_is_retriable_after_provisioning() {
        let failure = Failure::from_error(&verification_error(
            "npm error code ENOTCACHED",
            Path::new("verification.json"),
        ));
        assert_eq!(failure.kind, FailureKind::ExternalPrerequisite);
        assert!(failure.message.contains("npm error code ENOTCACHED"));
    }
}

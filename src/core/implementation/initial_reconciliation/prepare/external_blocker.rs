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
    let lower = error.to_ascii_lowercase();
    let next_action = if lower.contains("error nu1900:")
        && lower.contains("refreshed the public nuget audit cache and retried")
    {
        "Kool.ad/e refreshed the official public NuGet audit cache and retried verification, but the feed still failed. Provide controlled access to https://api.nuget.org/v3/index.json or the required approved NuGet audit source, then resume. Do not change repository build configuration to hide the audit failure."
    } else if lower.contains("error nu1900:") {
        "Kool.ad/e tried to refresh the official public NuGet audit cache but could not reach the feed. Provide controlled access to https://api.nuget.org/v3/index.json or the required approved NuGet audit source, then resume. Do not change repository build configuration to hide the audit failure."
    } else if lower.contains("error nu1301:") {
        "Kool.ad/e already uses the available host NuGet package cache, but the required package source could not be reached. Make the approved feed available to the host or populate that cache, then resume. Do not change repository build configuration to hide the unavailable feed."
    } else if lower.contains("out of memory") || lower.contains("outofmemoryexception") {
        "Provide a verification sandbox with sufficient memory, then resume. Do not change repository build configuration to work around this resource limit."
    } else {
        "Provide the required dependency cache or controlled package-feed access, then resume. Do not change repository build configuration to hide an unavailable audit or feed."
    };
    anyhow::Error::new(crate::core::implementation::status::FailureCause(
        Failure::new(
            FailureKind::ExternalPrerequisite,
            RecoveryDisposition::UserAction,
            format!(
                "## Waiting for environment\n\nRequired baseline verification failed because the sandbox lacks a dependency/feed or sufficient resources. The integration repository and both histories are preserved; no repository workaround was applied.\n\n### Evidence\n\n{error}\n\n### Next action(s)\n\n- Operator: {next_action}\n\nVerification log: {}",
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
        assert!(
            failure
                .message
                .contains("no repository workaround was applied")
        );
    }

    #[test]
    fn resource_blocker_requests_capacity_without_changing_repository_configuration() {
        let failure = Failure::from_error(&verification_error(
            "Microsoft.CSharp.Core.targets: error : Out of memory.",
            Path::new("verification.json"),
        ));
        assert!(failure.message.contains("sufficient memory"));
        assert!(
            failure
                .message
                .contains("Do not change repository build configuration")
        );
    }
}

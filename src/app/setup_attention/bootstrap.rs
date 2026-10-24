//! Classify first-run agent availability without relying on Docker.
//!
//! Discovery results are evidence, not authorization. A provider must have a
//! verified account, usable model catalog and safe context-only capability
//! before it may be selected to help with machine setup.

use crate::execution::contract::ReadinessState;
use crate::harness::discovery::{ModelCatalogStatus, ProbeResult, ProbeStatus};

use super::SetupIssue;

fn verified_for_bootstrap(report: &ProbeResult) -> bool {
    report.status == ProbeStatus::Available
        && report.binary_path.is_some()
        && report.version.is_some()
        && report.authentication == ReadinessState::Ready
        && report.execution_capability.safe_for_planning == ReadinessState::Ready
        && report.model_catalog_status == ModelCatalogStatus::Fresh
        && (!report.models.is_empty() || report.default_model.is_some())
}

/// Returns the first actionable CLI prerequisite, if any. Docker readiness is
/// intentionally not an input: the same result must hold for a rootful host.
pub(super) fn assess(reports: &[ProbeResult], selected: Option<&str>) -> Option<SetupIssue> {
    let verified = reports
        .iter()
        .filter(|report| verified_for_bootstrap(report))
        .collect::<Vec<_>>();
    if selected.is_some_and(|id| verified.iter().any(|report| report.provider_id == id)) {
        return None;
    }
    if !verified.is_empty() {
        return Some(SetupIssue::cli_select());
    }
    if !reports.iter().any(|report| report.binary_path.is_some()) {
        return Some(SetupIssue::cli_install());
    }
    if reports.iter().any(|report| {
        report.binary_path.is_some() && report.authentication == ReadinessState::Unavailable
    }) {
        return Some(SetupIssue::cli_authenticate());
    }
    Some(SetupIssue::cli_verify())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn probe(id: &str, installed: bool, auth: ReadinessState) -> ProbeResult {
        ProbeResult {
            provider_id: id.into(),
            binary_path: installed.then(|| format!("/usr/bin/{id}")),
            version: installed.then(|| "1.0.0".into()),
            authentication: auth,
            status: if auth == ReadinessState::Ready {
                ProbeStatus::Available
            } else if installed {
                ProbeStatus::Installed
            } else {
                ProbeStatus::Missing
            },
            model_catalog_status: ModelCatalogStatus::Fresh,
            models: vec!["safe-model".into()],
            execution_capability: crate::harness::discovery::ExecutionCapability {
                safe_for_planning: if auth == ReadinessState::Ready {
                    ReadinessState::Ready
                } else {
                    ReadinessState::Unknown
                },
                safe_for_implementation: ReadinessState::Unavailable,
                runtime_ready: ReadinessState::Unavailable,
            },
            ..ProbeResult::default()
        }
    }

    #[test]
    fn first_run_without_any_cli_requests_installation() {
        let reports = vec![probe("pi", false, ReadinessState::Unknown)];
        assert_eq!(assess(&reports, None).unwrap().id, "setup:cli:install");
    }

    #[test]
    fn rootful_docker_cannot_hide_a_valid_existing_agent() {
        let reports = vec![probe("codex", true, ReadinessState::Ready)];
        let docker_issue = || SetupIssue::docker("Rootful Linux rejected");
        let first = assess(&reports, Some("codex")).or_else(|| Some(docker_issue()));
        assert_eq!(first.unwrap().id, "setup:docker");
        assert_eq!(assess(&reports, None).unwrap().id, "setup:cli:select");
    }

    #[test]
    fn authentication_missing_is_distinct_from_installation() {
        let reports = vec![probe("pi", true, ReadinessState::Unavailable)];
        assert_eq!(assess(&reports, None).unwrap().id, "setup:cli:authenticate");
    }

    #[test]
    fn unknown_authentication_must_not_be_treated_as_ready() {
        let reports = vec![probe("copilot", true, ReadinessState::Unknown)];
        assert_eq!(assess(&reports, Some("copilot")).unwrap().id, "setup:cli:verify");
    }

    #[test]
    fn unverified_model_catalog_must_not_activate_bootstrap_agent() {
        let mut report = probe("claude", true, ReadinessState::Ready);
        report.model_catalog_status = ModelCatalogStatus::NotChecked;
        assert_eq!(assess(&[report], Some("claude")).unwrap().id, "setup:cli:verify");
    }

    #[test]
    fn selected_verified_agent_does_not_depend_on_docker_runtime() {
        let reports = vec![
            probe("pi", true, ReadinessState::Ready),
            probe("codex", true, ReadinessState::Ready),
        ];
        assert!(assess(&reports, Some("codex")).is_none());
        assert_eq!(assess(&reports, Some("missing")).unwrap().id, "setup:cli:select");
    }
}

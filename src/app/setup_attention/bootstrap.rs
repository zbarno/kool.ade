//! Classify first-run agent availability without relying on Docker.
//!
//! Discovery results are evidence, not authorization. A provider must have a
//! verified account, usable model catalog and safe context-only capability
//! before it may be selected to help with machine setup.

use crate::execution::contract::ReadinessState;
use crate::harness::discovery::{ModelCatalogStatus, ProbeResult, ProbeStatus};

use super::SetupIssue;
use super::catalog::ProviderIdV1;

fn verified_for_bootstrap(report: &ProbeResult, generation: u64) -> bool {
    report.status == ProbeStatus::Available
        && report.run_generation == generation
        && report.binary_path.is_some()
        && report.version.is_some()
        && report.authentication == ReadinessState::Ready
        && report.execution_capability.safe_for_planning == ReadinessState::Ready
        && report.model_catalog_status == ModelCatalogStatus::Fresh
        && (!report.models.is_empty() || report.default_model.is_some())
}

/// Returns the first actionable CLI prerequisite, if any. Docker readiness is
/// intentionally not an input: the same result must hold for a rootful host.
pub(super) fn assess(
    reports: &[ProbeResult],
    selected: Option<&str>,
    setup_provider: Option<&str>,
    generation: u64,
) -> Option<SetupIssue> {
    let mut chosen = Vec::new();
    for id in [setup_provider, selected].into_iter().flatten() {
        if !chosen.contains(&id) {
            chosen.push(id);
        }
    }
    let verified = reports
        .iter()
        .filter(|report| verified_for_bootstrap(report, generation))
        .collect::<Vec<_>>();
    if !chosen.is_empty() {
        for id in &chosen {
            if ProviderIdV1::from_probe_id(id).is_none() {
                return Some(selected_provider_verification_issue());
            }
            let Some(report) = reports.iter().find(|report| report.provider_id == *id) else {
                return Some(selected_provider_verification_issue());
            };
            if verified_for_bootstrap(report, generation) {
                continue;
            }
            if report.run_generation != generation {
                return Some(SetupIssue::cli_verify());
            }
            return Some(
                if report.binary_path.is_none() && report.status == ProbeStatus::Missing {
                    SetupIssue::cli_install()
                } else if report.authentication == ReadinessState::Unavailable {
                    SetupIssue::cli_authenticate()
                } else {
                    SetupIssue::cli_verify()
                },
            );
        }
        return if selected.is_some() {
            None
        } else {
            Some(SetupIssue::cli_select())
        };
    }
    if !verified.is_empty() {
        return Some(SetupIssue::cli_select());
    }
    if !reports.iter().any(|report| report.binary_path.is_some()) {
        // A crashed or timed-out probe is not evidence that a CLI is absent.
        return Some(
            if reports
                .iter()
                .any(|report| report.status == ProbeStatus::Failed)
            {
                SetupIssue::cli_verify()
            } else {
                SetupIssue::cli_install()
            },
        );
    }
    if reports.iter().any(|report| {
        report.binary_path.is_some() && report.authentication == ReadinessState::Unavailable
    }) {
        return Some(SetupIssue::cli_authenticate());
    }
    Some(SetupIssue::cli_verify())
}

fn selected_provider_verification_issue() -> SetupIssue {
    let mut issue = SetupIssue::cli_verify();
    issue.issue =
        "Kool.ad/e could not match the explicitly selected provider to current discovery results."
            .into();
    issue.recommendation = "Choose a supported provider in Coding Tools, then retry discovery to verify its account and model access.";
    issue.next_action = "Review the selected provider and retry the machine setup check.";
    issue
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
        assert_eq!(
            assess(&reports, None, None, 0).unwrap().id,
            "setup:cli:install"
        );
    }

    #[test]
    fn rootful_docker_cannot_hide_a_valid_existing_agent() {
        let reports = vec![probe("codex", true, ReadinessState::Ready)];
        let docker_issue = || SetupIssue::docker("Rootful Linux rejected");
        let first = assess(&reports, Some("codex"), None, 0).or_else(|| Some(docker_issue()));
        assert_eq!(first.unwrap().id, "setup:docker");
        assert_eq!(
            assess(&reports, None, None, 0).unwrap().id,
            "setup:cli:select"
        );
    }

    #[test]
    fn failed_probe_is_not_misreported_as_missing_software() {
        let mut broken = probe("codex", false, ReadinessState::Unknown);
        broken.status = ProbeStatus::Failed;
        assert_eq!(
            assess(&[broken], None, None, 0).unwrap().id,
            "setup:cli:verify"
        );
    }

    #[test]
    fn authentication_missing_is_distinct_from_installation() {
        let reports = vec![probe("pi", true, ReadinessState::Unavailable)];
        assert_eq!(
            assess(&reports, None, None, 0).unwrap().id,
            "setup:cli:authenticate"
        );
    }

    #[test]
    fn unknown_authentication_must_not_be_treated_as_ready() {
        let reports = vec![probe("copilot", true, ReadinessState::Unknown)];
        assert_eq!(
            assess(&reports, Some("copilot"), None, 0).unwrap().id,
            "setup:cli:verify"
        );
    }

    #[test]
    fn unverified_model_catalog_must_not_activate_bootstrap_agent() {
        let mut report = probe("claude", true, ReadinessState::Ready);
        report.model_catalog_status = ModelCatalogStatus::NotChecked;
        assert_eq!(
            assess(&[report], Some("claude"), None, 0).unwrap().id,
            "setup:cli:verify"
        );
    }

    #[test]
    fn selected_verified_agent_does_not_depend_on_docker_runtime() {
        let reports = vec![
            probe("pi", true, ReadinessState::Ready),
            probe("codex", true, ReadinessState::Ready),
        ];
        assert!(assess(&reports, Some("codex"), None, 0).is_none());
        assert_eq!(
            assess(&reports, Some("missing"), None, 0).unwrap().id,
            "setup:cli:verify"
        );
    }

    #[test]
    fn stale_provider_generation_cannot_qualify_for_bootstrap() {
        let mut report = probe("codex", true, ReadinessState::Ready);
        report.run_generation = 3;
        assert_eq!(
            assess(&[report], Some("codex"), None, 4).unwrap().id,
            "setup:cli:verify"
        );
    }

    #[test]
    fn guided_and_default_providers_are_both_assessed() {
        let mut default = probe("codex", true, ReadinessState::Unavailable);
        default.run_generation = 0;
        let guided = probe("pi", true, ReadinessState::Ready);
        assert_eq!(
            assess(&[guided, default], Some("codex"), Some("pi"), 0)
                .unwrap()
                .id,
            "setup:cli:authenticate"
        );
    }

    #[test]
    fn matching_guided_and_default_provider_is_assessed_once() {
        let report = probe("codex", true, ReadinessState::Unavailable);
        assert_eq!(
            assess(&[report], Some("codex"), Some("codex"), 0)
                .unwrap()
                .id,
            "setup:cli:authenticate"
        );
    }

    #[test]
    fn unknown_or_missing_selected_provider_returns_actionable_verification() {
        let unknown = assess(&[], Some("future-provider"), None, 0).unwrap();
        assert_eq!(unknown.id, "setup:cli:verify");
        assert!(unknown.issue.contains("could not match"));
        assert!(unknown.next_action.contains("Review the selected provider"));

        let missing = assess(&[], Some("codex"), None, 0).unwrap();
        assert_eq!(missing.id, "setup:cli:verify");
        assert!(missing.issue.contains("could not match"));
    }
}

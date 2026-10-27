use super::*;

#[test]
fn explicit_setup_choice_creates_one_provider_task_without_changing_default_route() {
    let mut probes = [
        "pi",
        "codex",
        "claude",
        "opencode",
        "copilot",
        "antigravity",
    ]
    .into_iter()
    .map(|id| {
        let mut probe = agent(ReadinessState::Unknown);
        probe.provider_id = id.into();
        probe.run_generation = 4;
        probe.status = ProbeStatus::Installed;
        probe
    })
    .collect::<Vec<_>>();
    probes[4].diagnostic_code =
        Some(crate::harness::discovery::ProbeDiagnosticCode::ExecutableInvalid);
    probes[4].binary_path = None;
    probes[4].status = ProbeStatus::Missing;
    let snapshot = super::assess_for_scan_with_git_and_bootstrap_provider(
        &probes,
        super::ProviderChoices {
            default_agent: None,
            guided_setup: Some("copilot"),
        },
        &docker(DockerEnvironmentState::NotInstalled, false),
        HostKind::Linux,
        7,
        4,
        super::GitProbeV1::not_run(),
    );
    let cli_findings = snapshot
        .findings
        .iter()
        .filter(|finding| finding.id.starts_with("setup:cli:"))
        .map(|finding| finding.id.as_str())
        .collect::<Vec<_>>();
    assert_eq!(cli_findings, ["setup:cli:copilot:path"]);
    assert_eq!(snapshot.setup_provider, Some(ProviderIdV1::Copilot));
    assert_eq!(snapshot.selected_provider, None);
    assert!(
        snapshot
            .checked_domains
            .iter()
            .any(|domain| domain == "cli:install")
    );
    assert!(snapshot.validate().is_ok());
}

#[test]
fn distinct_explicit_setup_and_default_choices_are_both_assessed_without_fanout() {
    let mut codex = agent(ReadinessState::Ready);
    codex.run_generation = 4;
    let mut pi = agent(ReadinessState::Unavailable);
    pi.provider_id = "pi".into();
    pi.run_generation = 4;
    pi.status = ProbeStatus::Installed;
    pi.execution_capability.safe_for_planning = ReadinessState::Unknown;
    let mut unselected = agent(ReadinessState::Unavailable);
    unselected.provider_id = "claude".into();
    unselected.run_generation = 4;
    unselected.status = ProbeStatus::Installed;
    unselected.execution_capability.safe_for_planning = ReadinessState::Unknown;

    let snapshot = super::assess_for_scan_with_git_and_bootstrap_provider(
        &[codex, pi, unselected],
        super::ProviderChoices {
            default_agent: Some("pi"),
            guided_setup: Some("codex"),
        },
        &docker(DockerEnvironmentState::NotInstalled, false),
        HostKind::Linux,
        7,
        4,
        super::GitProbeV1::not_run(),
    );
    let cli_ids = snapshot
        .findings
        .iter()
        .map(|finding| finding.id.as_str())
        .filter(|id| id.starts_with("setup:cli:"))
        .collect::<Vec<_>>();
    assert!(cli_ids.contains(&"setup:cli:pi:auth"));
    assert!(cli_ids.contains(&"setup:cli:pi:capability"));
    assert!(!cli_ids.iter().any(|id| id.starts_with("setup:cli:claude:")));
}

#[test]
fn matching_explicit_setup_and_default_choices_produce_one_provider_task() {
    let mut codex = agent(ReadinessState::Unavailable);
    codex.run_generation = 4;
    codex.status = ProbeStatus::Installed;
    codex.execution_capability.safe_for_planning = ReadinessState::Unknown;
    let snapshot = super::assess_for_scan_with_git_and_bootstrap_provider(
        &[codex],
        super::ProviderChoices {
            default_agent: Some("codex"),
            guided_setup: Some("codex"),
        },
        &docker(DockerEnvironmentState::NotInstalled, false),
        HostKind::Linux,
        7,
        4,
        super::GitProbeV1::not_run(),
    );
    let provider_ids = snapshot
        .findings
        .iter()
        .map(|finding| finding.id.as_str())
        .filter(|id| id.starts_with("setup:cli:codex:"))
        .collect::<Vec<_>>();
    assert_eq!(
        provider_ids,
        ["setup:cli:codex:auth", "setup:cli:codex:capability"]
    );
}

#[test]
fn unsupported_provider_version_has_a_distinct_task_and_failed_evidence() {
    let mut codex = agent(ReadinessState::Ready);
    codex.diagnostic_code =
        Some(crate::harness::discovery::ProbeDiagnosticCode::UnsupportedVersion);
    codex.run_generation = 5;
    let snapshot = super::assess_for_scan_with_git_and_bootstrap_provider(
        &[codex],
        super::ProviderChoices {
            default_agent: None,
            guided_setup: Some("codex"),
        },
        &docker(DockerEnvironmentState::NotInstalled, false),
        HostKind::Linux,
        8,
        5,
        super::GitProbeV1::not_run(),
    );
    assert!(
        snapshot
            .findings
            .iter()
            .any(|finding| finding.id == "setup:cli:codex:version")
    );
    assert!(snapshot.probe_evidence.iter().any(|evidence| {
        evidence.prerequisite.capability == super::PrerequisiteCapability::ProviderVersion
            && evidence.state == EvidenceState::Failed
            && evidence.code == super::EvidenceCode::ProviderVersionUnsupported
    }));
    assert!(
        !snapshot
            .checked_domains
            .iter()
            .any(|domain| domain == "cli:codex")
    );
    assert!(snapshot.validate().is_ok());
}

#[test]
fn invalid_executable_diagnostic_cannot_add_verified_provider_domain() {
    let mut codex = agent(ReadinessState::Ready);
    codex.run_generation = 5;
    codex.diagnostic_code = Some(crate::harness::discovery::ProbeDiagnosticCode::ExecutableInvalid);
    let snapshot = super::assess_for_scan_with_git_and_bootstrap_provider(
        &[codex],
        super::ProviderChoices {
            default_agent: None,
            guided_setup: Some("codex"),
        },
        &docker(DockerEnvironmentState::NotInstalled, false),
        HostKind::Linux,
        8,
        5,
        super::GitProbeV1::not_run(),
    );
    assert!(
        snapshot
            .findings
            .iter()
            .any(|finding| finding.id == "setup:cli:codex:path")
    );
    assert!(
        !snapshot
            .checked_domains
            .iter()
            .any(|domain| domain == "cli:codex")
    );
}

#[test]
fn independent_model_and_context_role_gaps_keep_distinct_task_ids() {
    let mut codex = agent(ReadinessState::Ready);
    codex.run_generation = 6;
    codex.model_catalog_status = ModelCatalogStatus::Unavailable;
    codex.models.clear();
    codex.default_model = None;
    codex.execution_capability.safe_for_planning = ReadinessState::Unavailable;
    let snapshot = super::assess_for_scan_with_git_and_bootstrap_provider(
        &[codex],
        super::ProviderChoices {
            default_agent: None,
            guided_setup: Some("codex"),
        },
        &docker(DockerEnvironmentState::NotInstalled, false),
        HostKind::Linux,
        9,
        6,
        super::GitProbeV1::not_run(),
    );
    let ids = snapshot
        .findings
        .iter()
        .map(|finding| finding.id.as_str())
        .filter(|id| id.starts_with("setup:cli:codex:"))
        .collect::<Vec<_>>();
    assert_eq!(
        ids,
        ["setup:cli:codex:capability", "setup:cli:codex:models"]
    );
    assert!(snapshot.validate().is_ok());
}

#[path = "providers/selected_setup.rs"]
mod selected_setup;

use super::*;

#[test]
fn claude_task_block_is_reported_separately_from_authentication_and_model_gaps() {
    let mut claude = agent(ReadinessState::Unknown);
    claude.provider_id = "claude".into();
    claude.run_generation = 4;
    claude.status = ProbeStatus::Installed;
    claude.model_catalog_status = crate::harness::discovery::ModelCatalogStatus::Stale;
    claude.execution_capability.safe_for_planning = ReadinessState::Unavailable;

    let snapshot = super::assess_for_scan_with_git_and_bootstrap_provider(
        &[claude],
        super::ProviderChoices {
            default_agent: None,
            guided_setup: Some("claude"),
        },
        &docker(DockerEnvironmentState::NotInstalled, false),
        HostKind::Linux,
        7,
        4,
        super::GitProbeV1::not_run(),
    );
    let findings = snapshot
        .findings
        .iter()
        .filter(|finding| finding.id.starts_with("setup:cli:claude:"))
        .collect::<Vec<_>>();

    assert!(
        findings
            .iter()
            .any(|finding| finding.id == "setup:cli:claude:auth-verify")
    );
    assert!(
        findings
            .iter()
            .any(|finding| finding.id == "setup:cli:claude:models")
    );
    assert!(findings.iter().any(|finding| {
        finding.id == "setup:cli:claude:capability" && finding.title.contains("safely sandboxed")
    }));
    assert!(snapshot.validate().is_ok());
}

#[test]
fn codex_task_block_guidance_names_the_selected_provider() {
    let mut codex = agent(ReadinessState::Ready);
    codex.provider_id = "codex".into();
    codex.run_generation = 4;
    codex.status = ProbeStatus::Available;
    codex.execution_capability.safe_for_planning = ReadinessState::Ready;

    let snapshot = super::assess_for_scan_with_git_and_bootstrap_provider(
        &[codex],
        super::ProviderChoices {
            default_agent: None,
            guided_setup: Some("codex"),
        },
        &docker(DockerEnvironmentState::NotInstalled, false),
        HostKind::Linux,
        7,
        4,
        super::GitProbeV1::not_run(),
    );
    let finding = snapshot
        .findings
        .iter()
        .find(|finding| finding.id == "setup:cli:codex:capability")
        .expect("Codex execution should have a provider-specific safety finding");

    assert!(finding.title.contains("Codex"));
    assert!(finding.next_step.contains("another provider"));
    assert!(finding.next_step.contains("Codex"));
    assert!(!finding.next_step.contains("Claude Code"));
    assert!(
        !snapshot
            .checked_domains
            .iter()
            .any(|domain| domain == "cli:codex")
    );
    assert!(snapshot.validate().is_ok());
}

#[test]
fn unknown_explicit_provider_returns_actionable_verification_task() {
    let snapshot = super::assess_for_scan_with_git_and_bootstrap_provider(
        &[],
        super::ProviderChoices {
            default_agent: None,
            guided_setup: Some("future-provider"),
        },
        &docker(DockerEnvironmentState::NotInstalled, false),
        HostKind::Linux,
        7,
        4,
        super::GitProbeV1::not_run(),
    );
    assert!(snapshot.findings.iter().any(|finding| {
        finding.id == "setup:cli:verify"
            && finding.next_step.contains("Choose a supported provider")
    }));
    assert!(!snapshot.findings.iter().any(|finding| {
        finding.id == "setup:cli:install" && finding.title == "Choose a coding agent to set up"
    }));
}

#[test]
fn selected_supported_provider_missing_from_scan_returns_verification_task() {
    let snapshot = super::assess_for_scan_with_git_and_bootstrap_provider(
        &[],
        super::ProviderChoices {
            default_agent: None,
            guided_setup: Some("codex"),
        },
        &docker(DockerEnvironmentState::NotInstalled, false),
        HostKind::Linux,
        7,
        4,
        super::GitProbeV1::not_run(),
    );
    assert!(snapshot.findings.iter().any(|finding| {
        finding.id == "setup:cli:codex:verify"
            && finding.next_step.contains("Retry Coding Tools discovery")
    }));
    assert!(
        !snapshot
            .checked_domains
            .iter()
            .any(|domain| domain == "cli:select" || domain == "cli:codex")
    );
    assert!(snapshot.validate().is_ok());
}

#[test]
fn checked_install_domain_accepts_a_current_default_scan_with_a_missing_setup_scan() {
    let mut codex = agent(ReadinessState::Ready);
    codex.provider_id = "codex".into();
    codex.run_generation = 4;
    let snapshot = super::assess_for_scan_with_git_and_bootstrap_provider(
        &[codex],
        super::ProviderChoices {
            default_agent: Some("codex"),
            guided_setup: Some("pi"),
        },
        &docker(DockerEnvironmentState::NotInstalled, false),
        HostKind::Linux,
        7,
        4,
        super::GitProbeV1::not_run(),
    );
    assert!(snapshot.findings.iter().any(|finding| {
        finding.id == "setup:cli:pi:verify"
            && finding.status == crate::app::setup_attention::catalog::FindingStatus::Unverified
    }));
    assert!(
        snapshot
            .checked_domains
            .iter()
            .any(|domain| domain == "cli:install")
    );
    assert!(
        !snapshot
            .checked_domains
            .iter()
            .any(|domain| domain == "cli:select")
    );
    assert!(
        !snapshot
            .checked_domains
            .iter()
            .any(|domain| domain == "cli:codex")
    );
    assert!(
        !snapshot
            .checked_domains
            .iter()
            .any(|domain| domain == "cli:pi")
    );
    assert!(snapshot.validate().is_ok());
}

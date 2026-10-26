use super::*;

#[test]
fn no_cli_produces_one_choice_not_six_install_tasks() {
    let snapshot = assess(
        &[],
        None,
        &docker(DockerEnvironmentState::NotInstalled, false),
        HostKind::Linux,
        4,
    );
    assert!(
        snapshot
            .findings
            .iter()
            .any(|finding| finding.id == "setup:cli:install")
    );
    assert_eq!(snapshot.findings.len(), 2);
}

#[test]
fn unchosen_installed_providers_do_not_fan_out_setup_tasks() {
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
        probe.status = ProbeStatus::Installed;
        probe
    })
    .collect::<Vec<_>>();
    probes[0].authentication = ReadinessState::Unavailable;
    probes[0].diagnostic_code =
        Some(crate::harness::discovery::ProbeDiagnosticCode::AuthenticationRequired);
    let snapshot = assess(
        &probes,
        None,
        &docker(DockerEnvironmentState::NotInstalled, false),
        HostKind::Linux,
        6,
    );
    let cli_findings = snapshot
        .findings
        .iter()
        .filter(|finding| finding.id.starts_with("setup:cli:"))
        .map(|finding| finding.id.as_str())
        .collect::<Vec<_>>();
    assert_eq!(cli_findings, ["setup:cli:install"]);
}

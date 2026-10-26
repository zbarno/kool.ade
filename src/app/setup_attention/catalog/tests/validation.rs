use super::*;

mod conflicts;

#[test]
fn snapshot_validation_rejects_a_platform_support_claim_without_evidence() {
    let mut snapshot = assess(
        &[],
        None,
        &docker(DockerEnvironmentState::NotInstalled, false),
        HostKind::MacOs,
        6,
    );
    assert!(snapshot.validate().is_ok());

    snapshot.capability_matrix.native_controller = SupportClaim::Experimental;
    assert!(snapshot.validate().is_err());
}

#[test]
fn snapshot_validation_binds_host_matrix_to_typed_probe_observations() {
    let mut snapshot = assess(
        &[],
        None,
        &docker(DockerEnvironmentState::NotInstalled, false),
        HostKind::Linux,
        7,
    );
    snapshot.capability_matrix.supported_runtime_architectures =
        vec![ImageArchitectureV1::Unsupported];
    assert!(snapshot.validate().is_err());

    let mut snapshot = assess(
        &[],
        None,
        &docker(DockerEnvironmentState::NotInstalled, false),
        HostKind::Linux,
        8,
    );
    snapshot.capability_matrix.linux_distribution_id =
        Some(crate::app::setup_attention::catalog::LinuxDistributionV1::Ubuntu);
    snapshot.capability_matrix.linux_distribution_version = Some(
        crate::app::setup_attention::catalog::LinuxDistributionVersionV1 {
            major: 24,
            minor: 4,
        },
    );
    snapshot.capability_matrix.linux_distribution =
        crate::app::setup_attention::catalog::EvidenceState::Ready;
    snapshot.capability_matrix.linux_distribution_support = Some(SupportClaim::Experimental);
    snapshot.capability_matrix.native_controller = SupportClaim::Experimental;
    let distribution = snapshot
        .probe_evidence
        .iter_mut()
        .find(|item| item.prerequisite.capability == PrerequisiteCapability::LinuxDistribution)
        .unwrap();
    distribution.state = EvidenceState::Unknown;
    distribution.code = crate::app::setup_attention::catalog::EvidenceCode::HostCapabilityNotProbed;
    distribution.observation = Some(
        crate::app::setup_attention::catalog::ProbeObservationV1::LinuxDistribution {
            distribution: None,
            version: None,
        },
    );
    let distribution_id = distribution.prerequisite.clone();
    if let Some(node) = snapshot
        .prerequisite_graph
        .iter_mut()
        .find(|node| node.id == distribution_id)
    {
        node.state = EvidenceState::Unknown;
    }
    assert!(snapshot.validate().is_err());
}

#[test]
fn snapshot_validation_rejects_probe_timestamps_outside_the_scan_interval() {
    let mut before_start = assess(
        &[],
        None,
        &docker(DockerEnvironmentState::NotInstalled, false),
        HostKind::Linux,
        50,
    );
    before_start.probe_evidence[0].observed_at_unix_ms = 49;
    assert!(before_start.validate().is_err());

    let mut after_completion = assess(
        &[],
        None,
        &docker(DockerEnvironmentState::NotInstalled, false),
        HostKind::Linux,
        50,
    );
    after_completion.probe_evidence[0].observed_at_unix_ms = 51;
    assert!(after_completion.validate().is_err());
}

#[test]
fn snapshot_validation_rejects_docker_evidence_from_another_platform() {
    let mut snapshot = assess(
        &[],
        None,
        &docker(DockerEnvironmentState::Ready, true),
        HostKind::Linux,
        9,
    );
    let replacements = snapshot
        .probe_evidence
        .iter_mut()
        .filter(|item| item.prerequisite.domain == PrerequisiteDomain::Docker)
        .map(|item| {
            let previous = item.prerequisite.clone();
            item.prerequisite.platform = HostKind::Windows;
            (previous, item.prerequisite.clone())
        })
        .collect::<Vec<_>>();
    for (previous, replacement) in replacements {
        let node = snapshot
            .prerequisite_graph
            .iter_mut()
            .find(|node| node.id == previous)
            .unwrap();
        node.id = replacement;
    }
    if !snapshot
        .checked_domains
        .iter()
        .any(|domain| domain == "docker")
    {
        snapshot.checked_domains.push("docker".into());
    }

    assert!(snapshot.validate().is_err());
}

#[test]
fn snapshot_validation_rejects_ready_docker_with_a_failure_code() {
    let mut snapshot = assess(
        &[],
        None,
        &docker(DockerEnvironmentState::Ready, true),
        HostKind::Linux,
        10,
    );
    let rootless = snapshot
        .probe_evidence
        .iter_mut()
        .find(|item| item.prerequisite.capability == PrerequisiteCapability::DockerRootless)
        .unwrap();
    rootless.code = EvidenceCode::DockerRootfulRejected;
    assert!(snapshot.validate().is_err());
}

#[test]
fn provider_scan_rejects_ready_authentication_with_a_missing_code() {
    let probes = [
        "pi",
        "codex",
        "claude",
        "opencode",
        "copilot",
        "antigravity",
    ]
    .into_iter()
    .map(|id| ProbeResult {
        provider_id: id.into(),
        ..agent(ReadinessState::Ready)
    })
    .collect::<Vec<_>>();
    let mut snapshot = assess(
        &probes,
        Some("codex"),
        &docker(DockerEnvironmentState::NotInstalled, false),
        HostKind::Linux,
        11,
    );
    let authentication = snapshot
        .probe_evidence
        .iter_mut()
        .find(|item| {
            item.prerequisite.capability == PrerequisiteCapability::ProviderAuthentication
                && item.prerequisite.subject == PrerequisiteSubject::Provider(ProviderIdV1::Codex)
        })
        .unwrap();
    authentication.code = EvidenceCode::AuthenticationRequired;
    assert!(snapshot.validate().is_err());
}

#[test]
fn readiness_domain_rejects_evidence_from_the_wrong_origin() {
    let mut snapshot = assess_for_scan_with_git(
        &[],
        None,
        &docker(DockerEnvironmentState::NotInstalled, false),
        HostKind::Linux,
        12,
        4,
        GitProbeV1 {
            executable_state: EvidenceState::Ready,
            executable_code: EvidenceCode::GitAvailable,
            identity_state: EvidenceState::Ready,
            identity_code: EvidenceCode::GitIdentityConfigured,
        },
    );
    let executable = snapshot
        .probe_evidence
        .iter_mut()
        .find(|item| item.prerequisite.capability == PrerequisiteCapability::GitExecutable)
        .unwrap();
    executable.origin = EvidenceOrigin::ProviderCli;
    assert!(snapshot.validate().is_err());
}

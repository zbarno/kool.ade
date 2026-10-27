use crate::app::setup_attention::catalog::{
    EvidenceCode, EvidenceOrigin, EvidenceState, HostCapabilityMatrixV1, HostKind,
    ImageArchitectureV1, MachineSnapshotV1, PrerequisiteCapability, PrerequisiteDomain,
    PrerequisiteId, PrerequisiteSubject, ProbeEvidenceV1, SupportClaim,
};

use super::desktop_docker_ready;

const SCAN_GENERATION: u64 = 3;

fn evidence(
    domain: PrerequisiteDomain,
    platform: HostKind,
    capability: PrerequisiteCapability,
    subject: PrerequisiteSubject,
    state: EvidenceState,
    code: EvidenceCode,
    origin: EvidenceOrigin,
) -> ProbeEvidenceV1 {
    ProbeEvidenceV1 {
        prerequisite: PrerequisiteId {
            domain,
            platform,
            capability,
            subject,
        },
        state,
        code,
        origin,
        scan_generation: SCAN_GENERATION,
        source_generation: None,
        observed_at_unix_ms: 10,
        observation: None,
    }
}

fn desktop_snapshot(host: HostKind) -> MachineSnapshotV1 {
    let probe_evidence = vec![
        evidence(
            PrerequisiteDomain::Host,
            host,
            PrerequisiteCapability::DockerDesktopTrust,
            PrerequisiteSubject::None,
            EvidenceState::Ready,
            EvidenceCode::DockerDesktopTrustVerified,
            EvidenceOrigin::HostRuntime,
        ),
        evidence(
            PrerequisiteDomain::Docker,
            host,
            PrerequisiteCapability::DockerEngine,
            PrerequisiteSubject::None,
            EvidenceState::Ready,
            EvidenceCode::DockerReady,
            EvidenceOrigin::DockerDiscovery,
        ),
        evidence(
            PrerequisiteDomain::Docker,
            host,
            PrerequisiteCapability::DockerEndpoint,
            PrerequisiteSubject::None,
            EvidenceState::Ready,
            EvidenceCode::DockerReady,
            EvidenceOrigin::DockerDiscovery,
        ),
        evidence(
            PrerequisiteDomain::Docker,
            host,
            PrerequisiteCapability::DockerDesktopVm,
            PrerequisiteSubject::None,
            EvidenceState::Ready,
            EvidenceCode::DockerDesktopVmDetected,
            EvidenceOrigin::DockerDiscovery,
        ),
        evidence(
            PrerequisiteDomain::Docker,
            host,
            PrerequisiteCapability::DockerCapacity,
            PrerequisiteSubject::None,
            EvidenceState::Ready,
            EvidenceCode::DockerReady,
            EvidenceOrigin::DockerDiscovery,
        ),
        evidence(
            PrerequisiteDomain::Docker,
            host,
            PrerequisiteCapability::DockerSecurity,
            PrerequisiteSubject::None,
            EvidenceState::Ready,
            EvidenceCode::DockerReady,
            EvidenceOrigin::DockerDiscovery,
        ),
    ];
    MachineSnapshotV1 {
        schema_version: MachineSnapshotV1::SCHEMA_VERSION,
        scan_generation: SCAN_GENERATION,
        scan_started_at_unix_ms: 10,
        host,
        setup_provider: None,
        selected_provider: None,
        checked_at_unix_ms: 10,
        capability_matrix: HostCapabilityMatrixV1 {
            native_controller: SupportClaim::Planned,
            observed_host_architecture: ImageArchitectureV1::Amd64,
            supported_runtime_architectures: vec![ImageArchitectureV1::Amd64],
            linux_distribution: EvidenceState::NotApplicable,
            linux_distribution_id: None,
            linux_distribution_version: None,
            linux_distribution_support: None,
            linux_uid_mappings: EvidenceState::NotApplicable,
            linux_service_manager: EvidenceState::NotApplicable,
            linux_package_manager: EvidenceState::NotApplicable,
            linux_rootless_mechanism: None,
            windows_wsl2: EvidenceState::Unknown,
            virtualization: EvidenceState::Unknown,
            docker_desktop_vm_trust: EvidenceState::Ready,
        },
        probe_evidence,
        prerequisite_graph: Vec::new(),
        findings: Vec::new(),
        checked_domains: Vec::new(),
    }
}

fn trust_evidence(snapshot: &mut MachineSnapshotV1) -> &mut ProbeEvidenceV1 {
    snapshot
        .probe_evidence
        .iter_mut()
        .find(|item| item.prerequisite.capability == PrerequisiteCapability::DockerDesktopTrust)
        .expect("desktop trust evidence")
}

fn docker_evidence(
    snapshot: &mut MachineSnapshotV1,
    capability: PrerequisiteCapability,
) -> &mut ProbeEvidenceV1 {
    snapshot
        .probe_evidence
        .iter_mut()
        .find(|item| {
            item.prerequisite.domain == PrerequisiteDomain::Docker
                && item.prerequisite.capability == capability
        })
        .expect("Docker evidence")
}

#[test]
fn desktop_readiness_requires_current_host_scoped_positive_trust() {
    for host in [HostKind::MacOs, HostKind::Windows] {
        assert!(desktop_docker_ready(&desktop_snapshot(host)));
    }
    assert!(!desktop_docker_ready(&desktop_snapshot(HostKind::Linux)));

    let mut missing = desktop_snapshot(HostKind::MacOs);
    missing
        .probe_evidence
        .retain(|item| item.prerequisite.capability != PrerequisiteCapability::DockerDesktopTrust);
    assert!(!desktop_docker_ready(&missing));

    let mut unknown = desktop_snapshot(HostKind::MacOs);
    trust_evidence(&mut unknown).state = EvidenceState::Unknown;
    unknown.capability_matrix.docker_desktop_vm_trust = EvidenceState::Unknown;
    assert!(!desktop_docker_ready(&unknown));

    let mut ready_but_unverified = desktop_snapshot(HostKind::MacOs);
    trust_evidence(&mut ready_but_unverified).code = EvidenceCode::DockerDesktopTrustUnverified;
    assert!(!desktop_docker_ready(&ready_but_unverified));

    let mut stale = desktop_snapshot(HostKind::MacOs);
    trust_evidence(&mut stale).scan_generation = SCAN_GENERATION - 1;
    assert!(!desktop_docker_ready(&stale));

    let mut wrong_platform = desktop_snapshot(HostKind::MacOs);
    trust_evidence(&mut wrong_platform).prerequisite.platform = HostKind::Linux;
    assert!(!desktop_docker_ready(&wrong_platform));

    let mut wrong_domain = desktop_snapshot(HostKind::MacOs);
    trust_evidence(&mut wrong_domain).prerequisite.domain = PrerequisiteDomain::Docker;
    assert!(!desktop_docker_ready(&wrong_domain));

    let mut wrong_subject = desktop_snapshot(HostKind::MacOs);
    trust_evidence(&mut wrong_subject).prerequisite.subject =
        PrerequisiteSubject::Provider(crate::app::setup_attention::catalog::ProviderIdV1::Pi);
    assert!(!desktop_docker_ready(&wrong_subject));

    let mut sourced_from_another_scan = desktop_snapshot(HostKind::MacOs);
    trust_evidence(&mut sourced_from_another_scan).source_generation = Some(SCAN_GENERATION);
    assert!(!desktop_docker_ready(&sourced_from_another_scan));

    let mut non_host_origin = desktop_snapshot(HostKind::MacOs);
    trust_evidence(&mut non_host_origin).origin = EvidenceOrigin::DockerDiscovery;
    assert!(!desktop_docker_ready(&non_host_origin));

    let mut matrix_disagrees = desktop_snapshot(HostKind::MacOs);
    matrix_disagrees.capability_matrix.docker_desktop_vm_trust = EvidenceState::Unknown;
    assert!(!desktop_docker_ready(&matrix_disagrees));

    for capability in [
        PrerequisiteCapability::DockerCapacity,
        PrerequisiteCapability::DockerSecurity,
    ] {
        let mut missing = desktop_snapshot(HostKind::MacOs);
        missing
            .probe_evidence
            .retain(|item| item.prerequisite.capability != capability);
        assert!(
            !desktop_docker_ready(&missing),
            "Desktop readiness requires {capability:?} evidence"
        );

        let mut stale = desktop_snapshot(HostKind::MacOs);
        docker_evidence(&mut stale, capability).scan_generation = SCAN_GENERATION - 1;
        assert!(
            !desktop_docker_ready(&stale),
            "Desktop readiness requires current {capability:?} evidence"
        );

        let mut invalid_code = desktop_snapshot(HostKind::MacOs);
        docker_evidence(&mut invalid_code, capability).code =
            EvidenceCode::DockerCapacityUnavailable;
        assert!(
            !desktop_docker_ready(&invalid_code),
            "Desktop readiness requires positive {capability:?} evidence"
        );
    }
}

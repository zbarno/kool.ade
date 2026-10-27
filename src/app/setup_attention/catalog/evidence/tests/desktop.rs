use super::*;

#[test]
fn rootless_and_desktop_vm_remediation_are_not_blocked_by_the_engine_they_enable() {
    let rootful = super::super::super::assess_for_scan(
        &[],
        None,
        &docker(DockerEnvironmentState::RootfulLinuxRejected, false),
        HostKind::Linux,
        300,
        15,
    );
    let rootless = rootful
        .prerequisite_graph
        .iter()
        .find(|node| node.id.capability == PrerequisiteCapability::DockerRootless)
        .unwrap();
    assert_eq!(
        rootless.remediation,
        super::super::super::RemediationEligibility::Eligible
    );

    let desktop = super::super::super::assess_for_scan(
        &[],
        None,
        &docker(DockerEnvironmentState::DesktopVmRequired, false),
        HostKind::MacOs,
        301,
        16,
    );
    let vm = desktop
        .prerequisite_graph
        .iter()
        .find(|node| node.id.capability == PrerequisiteCapability::DockerDesktopVm)
        .unwrap();
    assert_eq!(
        vm.remediation,
        super::super::super::RemediationEligibility::Eligible
    );
}

#[test]
fn unknown_desktop_transport_does_not_claim_desktop_readiness() {
    let mut status = docker(DockerEnvironmentState::Ready, true);
    status.desktop_vm = true;
    status
        .available_images
        .push("locked-general-reference".into());
    let snapshot =
        super::super::super::assess_for_scan(&[], None, &status, HostKind::MacOs, 302, 17);
    assert!(
        !snapshot
            .checked_domains
            .iter()
            .any(|domain| domain == "docker:desktop")
    );
    assert!(
        !snapshot
            .checked_domains
            .iter()
            .any(|domain| domain == "docker")
    );
    assert!(snapshot.probe_evidence.iter().any(|evidence| {
        evidence.prerequisite.capability == PrerequisiteCapability::DockerSecurity
            && evidence.state == EvidenceState::Unknown
    }));
    assert!(snapshot.probe_evidence.iter().any(|evidence| {
        evidence.prerequisite.domain == PrerequisiteDomain::Image
            && evidence.state == EvidenceState::Blocked
    }));
    assert!(snapshot.validate().is_ok());
}

#[test]
fn verified_desktop_transport_adds_positive_host_trust_evidence() {
    let mut status = docker(DockerEnvironmentState::Ready, true);
    status.desktop_vm = true;
    status.desktop_transport_verified = true;
    status.desktop_vm_capacity_verified = true;
    status.workspace_staging_available = true;
    status.disk_available_bytes = Some(1_000_000_000);
    status.cpu_limits = Some(true);
    status.memory_limits = Some(true);
    status.pids_limit = Some(true);
    status
        .available_images
        .push("locked-general-reference".into());

    let snapshot =
        super::super::super::assess_for_scan(&[], None, &status, HostKind::MacOs, 303, 18);

    let trust = snapshot
        .probe_evidence
        .iter()
        .find(|item| {
            item.prerequisite.domain == PrerequisiteDomain::Host
                && item.prerequisite.platform == HostKind::MacOs
                && item.prerequisite.capability == PrerequisiteCapability::DockerDesktopTrust
        })
        .expect("Desktop trust fact");
    assert_eq!(trust.state, EvidenceState::Ready);
    assert_eq!(trust.code, EvidenceCode::DockerDesktopTrustVerified);
    assert_eq!(trust.origin, EvidenceOrigin::HostRuntime);
    assert!(snapshot.probe_evidence.iter().any(|evidence| {
        evidence.prerequisite.capability == PrerequisiteCapability::DockerCapacity
            && evidence.state == EvidenceState::Ready
    }));
    assert!(snapshot.probe_evidence.iter().any(|evidence| {
        evidence.prerequisite.capability == PrerequisiteCapability::DockerSecurity
            && evidence.state == EvidenceState::Ready
    }));
    assert!(
        snapshot
            .checked_domains
            .iter()
            .any(|domain| domain == "docker:desktop")
    );
    assert!(snapshot.validate().is_ok());

    let mut unverified_capacity = status.clone();
    unverified_capacity.desktop_vm_capacity_verified = false;
    unverified_capacity.disk_available_bytes = None;
    let snapshot = super::super::super::assess_for_scan(
        &[],
        None,
        &unverified_capacity,
        HostKind::MacOs,
        304,
        19,
    );
    assert!(
        !snapshot
            .checked_domains
            .iter()
            .any(|domain| domain == "docker:desktop")
    );
    assert!(snapshot.probe_evidence.iter().any(|evidence| {
        evidence.prerequisite.capability == PrerequisiteCapability::DockerCapacity
            && evidence.state != EvidenceState::Ready
    }));

    let mut missing_staging = status;
    missing_staging.workspace_staging_available = false;
    let snapshot = super::super::super::assess_for_scan(
        &[],
        None,
        &missing_staging,
        HostKind::Windows,
        305,
        20,
    );
    assert!(
        !snapshot
            .checked_domains
            .iter()
            .any(|domain| domain == "docker:desktop")
    );
    assert!(snapshot.probe_evidence.iter().any(|evidence| {
        evidence.prerequisite.capability == PrerequisiteCapability::DockerCapacity
            && evidence.state != EvidenceState::Ready
    }));
}

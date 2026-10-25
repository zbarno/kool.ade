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
fn unknown_desktop_trust_blocks_security_and_image_readiness() {
    let mut status = docker(DockerEnvironmentState::Ready, true);
    status.desktop_vm = true;
    status
        .available_images
        .push("locked-general-reference".into());
    let snapshot =
        super::super::super::assess_for_scan(&[], None, &status, HostKind::MacOs, 302, 17);
    assert!(
        snapshot
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

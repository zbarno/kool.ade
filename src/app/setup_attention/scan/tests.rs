use super::*;
use crate::execution::docker::environment::{DockerEnvironmentState, DockerEnvironmentStatus};

#[path = "tests/vm_evidence.rs"]
mod vm_evidence;

fn ready_engine() -> crate::execution::docker::environment::DockerEnvironmentStatus {
    DockerEnvironmentStatus {
        state: DockerEnvironmentState::MissingApprovedImage,
        engine_ready: true,
        message: "synthetic ready engine".into(),
        desktop_vm: false,
        desktop_transport_verified: false,
        desktop_vm_capacity_verified: false,
        workspace_staging_available: true,
        daemon_version: Some("28.0.0".into()),
        architecture: Some("amd64".into()),
        rootless: Some(true),
        cpu_limits: Some(true),
        memory_limits: Some(true),
        pids_limit: Some(true),
        cgroups_supported: Some(true),
        available_images: Vec::new(),
        disk_available_bytes: Some(1_000_000_000),
        endpoint_fingerprint: Some("synthetic-endpoint".into()),
        daemon_identity_fingerprint: Some("d".repeat(64)),
    }
}

fn snapshot(
    git: catalog::EvidenceState,
    code: catalog::EvidenceCode,
) -> catalog::MachineSnapshotV1 {
    catalog::assess_for_scan_with_git(
        &[],
        None,
        &ready_engine(),
        catalog::HostKind::Linux,
        100,
        1,
        catalog::GitProbeV1 {
            executable_state: git,
            executable_code: code,
            identity_state: catalog::EvidenceState::Blocked,
            identity_code: catalog::EvidenceCode::ParentPrerequisiteUnavailable,
        },
    )
}

#[test]
fn kvm_checked_domain_requires_all_current_host_prerequisites() {
    let mut ready = snapshot(
        catalog::EvidenceState::Ready,
        catalog::EvidenceCode::GitAvailable,
    );
    ready.checked_domains.push("computer:kvm".into());
    ready.probe_evidence.push(catalog::ProbeEvidenceV1 {
        prerequisite: catalog::PrerequisiteId {
            domain: catalog::PrerequisiteDomain::Host,
            platform: catalog::HostKind::Linux,
            capability: catalog::PrerequisiteCapability::LinuxKvmDevice,
            subject: catalog::PrerequisiteSubject::None,
        },
        state: catalog::EvidenceState::Ready,
        code: catalog::EvidenceCode::LinuxKvmDeviceAccessible,
        origin: catalog::EvidenceOrigin::HostRuntime,
        scan_generation: ready.scan_generation,
        source_generation: None,
        observed_at_unix_ms: ready.checked_at_unix_ms,
        observation: Some(catalog::ProbeObservationV1::LinuxKvmDevice { accessible: true }),
    });
    let kvm_id = ready.probe_evidence.last().unwrap().prerequisite.clone();
    ready.prerequisite_graph.push(catalog::PrerequisiteNodeV1 {
        id: kvm_id,
        state: catalog::EvidenceState::Ready,
        depends_on: Vec::new(),
        remediation: catalog::RemediationEligibility::Unverified,
    });
    let kvm_evidence_index = ready
        .probe_evidence
        .iter()
        .position(|item| {
            item.prerequisite.capability == catalog::PrerequisiteCapability::LinuxKvmDevice
        })
        .unwrap();
    // Device-open proof alone is insufficient to claim the host is ready; the
    // checked domain also needs architecture, kernel, namespace, and CPU evidence.
    assert!(ready.validate().is_err());

    let mut missing_proof = ready.clone();
    missing_proof.probe_evidence.retain(|item| {
        item.prerequisite.capability != catalog::PrerequisiteCapability::LinuxKvmDevice
    });
    missing_proof
        .prerequisite_graph
        .retain(|node| node.id.capability != catalog::PrerequisiteCapability::LinuxKvmDevice);
    assert!(missing_proof.validate().is_err());

    let mut false_proof = ready.clone();
    false_proof.probe_evidence[kvm_evidence_index].observation =
        Some(catalog::ProbeObservationV1::LinuxKvmDevice { accessible: false });
    assert!(false_proof.validate().is_err());

    let mut stale_proof = ready;
    stale_proof.probe_evidence[kvm_evidence_index].scan_generation -= 1;
    assert!(stale_proof.validate().is_err());
}

#[test]
fn vm_resource_evidence_must_match_the_current_host_and_fixed_guest_budget() {
    let mut machine = snapshot(
        catalog::EvidenceState::Ready,
        catalog::EvidenceCode::GitAvailable,
    );
    let budget = crate::computer_use::MINIMUM_DISPOSABLE_VM_BUDGET_V1;
    let evidence = catalog::ProbeEvidenceV1 {
        prerequisite: catalog::PrerequisiteId {
            domain: catalog::PrerequisiteDomain::Host,
            platform: catalog::HostKind::Linux,
            capability: catalog::PrerequisiteCapability::LinuxVmResources,
            subject: catalog::PrerequisiteSubject::None,
        },
        state: catalog::EvidenceState::Ready,
        code: catalog::EvidenceCode::LinuxVmResourcesAvailable,
        origin: catalog::EvidenceOrigin::HostRuntime,
        scan_generation: machine.scan_generation,
        source_generation: None,
        observed_at_unix_ms: machine.checked_at_unix_ms,
        observation: Some(catalog::ProbeObservationV1::LinuxVmResources {
            available_memory_bytes: Some(budget.memory_bytes),
            available_storage_bytes: Some(budget.disk_bytes),
            required_memory_bytes: budget.memory_bytes,
            required_storage_bytes: budget.disk_bytes,
            filesystem_family: Some(catalog::LinuxVmStorageFilesystemV1::Ext),
        }),
    };
    machine
        .prerequisite_graph
        .push(catalog::PrerequisiteNodeV1 {
            id: evidence.prerequisite.clone(),
            state: evidence.state,
            depends_on: Vec::new(),
            remediation: catalog::RemediationEligibility::Unverified,
        });
    machine.probe_evidence.push(evidence);
    assert!(machine.validate().is_ok());

    let mut forged_budget = machine.clone();
    let Some(catalog::ProbeObservationV1::LinuxVmResources {
        required_memory_bytes,
        ..
    }) = forged_budget
        .probe_evidence
        .last_mut()
        .unwrap()
        .observation
        .as_mut()
    else {
        unreachable!();
    };
    *required_memory_bytes += 1;
    assert!(forged_budget.validate().is_err());

    let mut filesystem_without_storage = machine;
    let resource = filesystem_without_storage
        .probe_evidence
        .last_mut()
        .unwrap();
    if let Some(catalog::ProbeObservationV1::LinuxVmResources {
        available_storage_bytes,
        ..
    }) = resource.observation.as_mut()
    {
        *available_storage_bytes = None;
    }
    resource.state = catalog::EvidenceState::Unknown;
    resource.code = catalog::EvidenceCode::LinuxVmResourcesUnknown;
    let resource_id = resource.prerequisite.clone();
    filesystem_without_storage
        .prerequisite_graph
        .iter_mut()
        .find(|node| node.id == resource_id)
        .unwrap()
        .state = catalog::EvidenceState::Unknown;
    assert!(filesystem_without_storage.validate().is_err());
}

#[test]
fn missing_general_image_or_git_cannot_produce_a_ready_result() {
    let general_image = snapshot(
        catalog::EvidenceState::Ready,
        catalog::EvidenceCode::GitAvailable,
    );
    assert!(
        general_image
            .findings
            .iter()
            .any(|finding| finding.id.starts_with("setup:images:general:"))
    );
    assert_eq!(
        issue_for_readiness(None, true, &general_image).unwrap().id,
        "setup:bootstrap"
    );

    let git_missing = snapshot(
        catalog::EvidenceState::Missing,
        catalog::EvidenceCode::GitMissing,
    );
    assert_eq!(
        issue_for_readiness(None, true, &git_missing).unwrap().id,
        "setup:bootstrap"
    );
}

#[test]
fn unsupported_computer_use_does_not_block_ready_docker_work() {
    let mut ready = snapshot(
        catalog::EvidenceState::Ready,
        catalog::EvidenceCode::GitAvailable,
    );
    ready.findings.retain(|finding| {
        !finding.id.starts_with("setup:images:") && !finding.id.starts_with("setup:git:")
    });
    ready.findings.push(catalog::Finding {
        id: "setup:computer:image:amd64".into(),
        title: "Unsupported computer image".into(),
        explanation: "synthetic unsupported host".into(),
        next_step: "Use a supported host".into(),
        status: catalog::FindingStatus::Unsupported,
        prerequisites: Vec::new(),
    });
    assert!(issue_for_readiness(None, true, &ready).is_none());
}

/// Production boundary: the final worker check compares the daemon identity and
/// selected provider before committing the local task snapshot. This pure test
/// exercises the identity predicate without Docker, a network request, or disk
/// access; setup cost is negligible.
#[test]
fn daemon_or_provider_switch_rejects_the_scan_identity() {
    let docker = ready_engine();
    let result = crate::app::setup_attention::SetupScanResult {
        generation: 3,
        issue: None,
        snapshot: None,
        endpoint_fingerprint: Some("synthetic-endpoint".into()),
        daemon_identity_fingerprint: docker.daemon_identity_fingerprint.clone(),
        docker_readiness_fingerprint: Some(docker.readiness_fingerprint()),
        docker_local_execution_ready: false,
        setup_provider: None,
        selected_provider: None,
        cancelled: false,
    };
    let matches =
        |result: &crate::app::setup_attention::SetupScanResult,
         setup_provider,
         selected,
         before,
         after,
         docker: &crate::execution::docker::environment::DockerEnvironmentStatus| {
            super::verification_identity_matches(
                result,
                setup_provider,
                selected,
                before,
                after,
                docker,
            )
        };
    assert!(matches(
        &result,
        None,
        None,
        Some("synthetic-endpoint".into()),
        Some("synthetic-endpoint".into()),
        &docker,
    ));
    assert!(!matches(
        &result,
        Some(catalog::ProviderIdV1::Codex),
        None,
        Some("synthetic-endpoint".into()),
        Some("synthetic-endpoint".into()),
        &docker,
    ));
    let mut changed_readiness = docker.clone();
    changed_readiness.cgroups_supported = Some(false);
    assert!(!matches(
        &result,
        None,
        None,
        Some("synthetic-endpoint".into()),
        Some("synthetic-endpoint".into()),
        &changed_readiness,
    ));
    let mut switched_daemon = docker;
    switched_daemon.daemon_identity_fingerprint = Some("different-daemon".into());
    assert!(!matches(
        &result,
        None,
        None,
        Some("synthetic-endpoint".into()),
        Some("synthetic-endpoint".into()),
        &switched_daemon,
    ));
}

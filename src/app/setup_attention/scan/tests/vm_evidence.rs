use super::*;

fn evidence(
    snapshot: &catalog::MachineSnapshotV1,
    capability: catalog::PrerequisiteCapability,
    state: catalog::EvidenceState,
    code: catalog::EvidenceCode,
    observation: catalog::ProbeObservationV1,
) -> catalog::ProbeEvidenceV1 {
    catalog::ProbeEvidenceV1 {
        prerequisite: catalog::PrerequisiteId {
            domain: catalog::PrerequisiteDomain::Host,
            platform: catalog::HostKind::Linux,
            capability,
            subject: catalog::PrerequisiteSubject::None,
        },
        state,
        code,
        origin: catalog::EvidenceOrigin::HostRuntime,
        scan_generation: snapshot.scan_generation,
        source_generation: None,
        observed_at_unix_ms: snapshot.checked_at_unix_ms,
        observation: Some(observation),
    }
}

fn add(snapshot: &mut catalog::MachineSnapshotV1, evidence: catalog::ProbeEvidenceV1) {
    snapshot
        .prerequisite_graph
        .push(catalog::PrerequisiteNodeV1 {
            id: evidence.prerequisite.clone(),
            state: evidence.state,
            depends_on: Vec::new(),
            remediation: catalog::RemediationEligibility::Unverified,
        });
    snapshot.probe_evidence.push(evidence);
}

#[test]
fn qemu_detection_requires_verified_launcher_containment() {
    let mut machine = super::snapshot(
        catalog::EvidenceState::Ready,
        catalog::EvidenceCode::GitAvailable,
    );
    let qemu = evidence(
        &machine,
        catalog::PrerequisiteCapability::LinuxQemu,
        catalog::EvidenceState::Ready,
        catalog::EvidenceCode::LinuxQemuDetected,
        catalog::ProbeObservationV1::LinuxQemu {
            version: Some("QEMU emulator version 9.2.0".into()),
            kvm_acceleration: Some(false),
            qemu_image_tool_available: Some(true),
            qemu_launcher_contained: Some(true),
        },
    );
    add(&mut machine, qemu);
    assert!(machine.validate().is_ok());

    let mut untrusted_launcher = machine.clone();
    if let Some(catalog::ProbeObservationV1::LinuxQemu {
        qemu_launcher_contained,
        ..
    }) = untrusted_launcher
        .probe_evidence
        .last_mut()
        .unwrap()
        .observation
        .as_mut()
    {
        *qemu_launcher_contained = Some(false);
    }
    assert!(untrusted_launcher.validate().is_err());

    let mut malformed = machine.clone();
    if let Some(catalog::ProbeObservationV1::LinuxQemu { version, .. }) = malformed
        .probe_evidence
        .last_mut()
        .unwrap()
        .observation
        .as_mut()
    {
        *version = Some("QEMU emulator version nightly".into());
    }
    assert!(malformed.validate().is_err());

    let mut forged = machine;
    forged.probe_evidence.last_mut().unwrap().state = catalog::EvidenceState::Missing;
    assert!(forged.validate().is_err());
}

#[test]
fn permission_denied_kvm_is_typed_failed_evidence_not_ready() {
    let mut machine = super::snapshot(
        catalog::EvidenceState::Ready,
        catalog::EvidenceCode::GitAvailable,
    );
    let kvm = evidence(
        &machine,
        catalog::PrerequisiteCapability::LinuxKvmDevice,
        catalog::EvidenceState::Failed,
        catalog::EvidenceCode::LinuxKvmDevicePermissionDenied,
        catalog::ProbeObservationV1::LinuxKvmDevice { accessible: false },
    );
    add(&mut machine, kvm);
    assert!(machine.validate().is_ok());

    let mut forged = machine;
    forged.probe_evidence.last_mut().unwrap().state = catalog::EvidenceState::Ready;
    assert!(forged.validate().is_err());
}

#[test]
fn cpu_flag_alone_cannot_validate_a_supported_linux_virtualization_claim() {
    let mut machine = super::snapshot(
        catalog::EvidenceState::Ready,
        catalog::EvidenceCode::GitAvailable,
    );
    let virtualization = evidence(
        &machine,
        catalog::PrerequisiteCapability::LinuxVirtualization,
        catalog::EvidenceState::Unknown,
        catalog::EvidenceCode::LinuxVirtualizationUnknown,
        catalog::ProbeObservationV1::LinuxVirtualization {
            cpu_virtualization_flag: Some(true),
            nested_hypervisor: None,
        },
    );
    add(&mut machine, virtualization);
    assert!(machine.validate().is_ok());

    let mut forged = machine;
    forged.probe_evidence.last_mut().unwrap().state = catalog::EvidenceState::Ready;
    forged.probe_evidence.last_mut().unwrap().code =
        catalog::EvidenceCode::LinuxVirtualizationAvailable;
    assert!(forged.validate().is_err());
}

/// Exercises the Linux VM probe-to-snapshot helper with live host observations.
/// Encoder unit tests cannot prove independent samples are attached and validate
/// together. This calls the helper directly with a synthetic scan identity; it does
/// not exercise `scan()` persistence glue. It reads Linux proc/device/filesystem
/// state and runs fixed QEMU probes, but starts no VM and needs no Docker or network.
#[test]
fn linux_vm_probe_helper_persists_current_typed_evidence_and_dependencies() {
    let mut machine = super::snapshot(
        catalog::EvidenceState::Ready,
        catalog::EvidenceCode::GitAvailable,
    );
    let host_evidence = machine.probe_evidence.clone();
    let architecture = machine.capability_matrix.observed_host_architecture;
    let generation = machine.scan_generation;
    super::super::vm_readiness::append_findings(
        &mut machine,
        catalog::HostKind::Linux,
        architecture,
        &host_evidence,
        generation,
    );
    for capability in [
        catalog::PrerequisiteCapability::LinuxVirtualization,
        catalog::PrerequisiteCapability::LinuxKvmDevice,
        catalog::PrerequisiteCapability::LinuxQemu,
        catalog::PrerequisiteCapability::LinuxVmResources,
    ] {
        let evidence = machine
            .probe_evidence
            .iter()
            .find(|item| item.prerequisite.capability == capability)
            .expect("Linux VM probe should persist typed evidence");
        assert_eq!(evidence.scan_generation, generation);
        assert!(evidence.observed_at_unix_ms <= machine.checked_at_unix_ms);
    }
    for (capability, expected_state) in [
        (
            catalog::PrerequisiteCapability::ComputerGuestImage,
            catalog::EvidenceState::Unknown,
        ),
        (
            catalog::PrerequisiteCapability::ComputerOverlay,
            catalog::EvidenceState::Unknown,
        ),
        (
            catalog::PrerequisiteCapability::ComputerNetworkGateway,
            catalog::EvidenceState::Unsupported,
        ),
        (
            catalog::PrerequisiteCapability::ComputerGraphicsTransport,
            catalog::EvidenceState::Unsupported,
        ),
        (
            catalog::PrerequisiteCapability::ComputerEncryptionKeyService,
            catalog::EvidenceState::Unsupported,
        ),
        (
            catalog::PrerequisiteCapability::ComputerGuestProtocol,
            catalog::EvidenceState::Unknown,
        ),
    ] {
        let evidence = machine
            .probe_evidence
            .iter()
            .find(|item| item.prerequisite.capability == capability)
            .expect("component inventory should persist typed evidence");
        assert_eq!(evidence.state, expected_state, "{capability:?}");
        assert_eq!(
            evidence.code,
            if capability == catalog::PrerequisiteCapability::ComputerGuestImage {
                catalog::EvidenceCode::ComputerGuestImageCatalogUnverified
            } else {
                catalog::EvidenceCode::ComputerUseReadinessProbeNotIntegrated
            }
        );
        assert_eq!(
            evidence.origin,
            catalog::EvidenceOrigin::ApplicationInventory
        );
        assert_eq!(evidence.scan_generation, generation);
    }
    let qemu_node = machine
        .prerequisite_graph
        .iter()
        .find(|node| node.id.capability == catalog::PrerequisiteCapability::LinuxQemu)
        .unwrap();
    assert!(qemu_node.depends_on.is_empty());
    assert!(machine.validate().is_ok());
}

/// Exercises the production probe-to-snapshot boundary on an unsupported Linux
/// architecture. A fact-level test cannot prove Unsupported evidence is emitted
/// without panicking and survives snapshot validation. It reads fixed Linux host
/// probes and resource metadata, skips QEMU execution, and needs no Docker/network.
#[test]
fn arm64_linux_emits_valid_unsupported_kvm_and_qemu_evidence() {
    let mut machine = super::snapshot(
        catalog::EvidenceState::Ready,
        catalog::EvidenceCode::GitAvailable,
    );
    machine.capability_matrix.observed_host_architecture = catalog::ImageArchitectureV1::Arm64;
    machine.capability_matrix.native_controller = catalog::SupportClaim::Unverified;
    let architecture = machine
        .probe_evidence
        .iter_mut()
        .find(|item| {
            item.prerequisite.capability == catalog::PrerequisiteCapability::HostArchitecture
        })
        .expect("host architecture evidence");
    if let Some(catalog::ProbeObservationV1::HostArchitecture { observed, .. }) =
        architecture.observation.as_mut()
    {
        *observed = catalog::ImageArchitectureV1::Arm64;
    }
    let rootless_recipe = machine
        .probe_evidence
        .iter_mut()
        .find(|item| {
            item.prerequisite.capability == catalog::PrerequisiteCapability::LinuxRootlessRecipe
        })
        .expect("rootless recipe evidence");
    if let Some(catalog::ProbeObservationV1::LinuxRootlessRecipe { candidate, .. }) =
        rootless_recipe.observation.as_mut()
    {
        *candidate = None;
    }
    rootless_recipe.state = catalog::EvidenceState::Unknown;
    rootless_recipe.code = catalog::EvidenceCode::LinuxRootlessRecipeUnverified;
    let rootless_recipe_node = machine
        .prerequisite_graph
        .iter_mut()
        .find(|node| node.id.capability == catalog::PrerequisiteCapability::LinuxRootlessRecipe)
        .expect("rootless recipe graph node");
    rootless_recipe_node.state = catalog::EvidenceState::Unknown;
    let host_evidence = machine.probe_evidence.clone();
    let generation = machine.scan_generation;
    super::super::vm_readiness::append_findings(
        &mut machine,
        catalog::HostKind::Linux,
        catalog::ImageArchitectureV1::Arm64,
        &host_evidence,
        generation,
    );
    for capability in [
        catalog::PrerequisiteCapability::LinuxKvmDevice,
        catalog::PrerequisiteCapability::LinuxQemu,
    ] {
        let item = machine
            .probe_evidence
            .iter()
            .find(|item| item.prerequisite.capability == capability)
            .expect("unsupported Linux VM evidence should be emitted");
        assert_eq!(item.state, catalog::EvidenceState::Unsupported);
        assert_eq!(item.code, catalog::EvidenceCode::LinuxProbeUnsupported);
    }
    assert!(machine.validate().is_ok(), "{:?}", machine.validate());
}

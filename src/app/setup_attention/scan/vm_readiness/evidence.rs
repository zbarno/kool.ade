use super::{Check, QemuCapabilities, VmFacts, resources::ResourceCapacity};
use crate::app::setup_attention::catalog::*;

pub(super) fn append(
    snapshot: &mut MachineSnapshotV1,
    facts: &VmFacts,
    image_probe: super::guest_image::GuestImageCatalogProbe,
) {
    let virtualization = virtualization_evidence(
        snapshot,
        &facts.cpu_virtualization,
        &facts.nested_virtualization,
    );
    let kvm = kvm_evidence(snapshot, &facts.kvm);
    let qemu = qemu_evidence(snapshot, &facts.qemu, facts.qemu_launcher_containment);
    let resources = resource_evidence(snapshot, facts.resources);
    push(snapshot, virtualization, &[]);
    push(
        snapshot,
        kvm,
        &[
            PrerequisiteCapability::LinuxVirtualization,
            PrerequisiteCapability::LinuxKernel,
            PrerequisiteCapability::LinuxUserNamespaces,
        ],
    );
    push(snapshot, qemu, &[]);
    push(snapshot, resources, &[]);
    push_guest_image(snapshot, image_probe);
    push_component(
        snapshot,
        PrerequisiteCapability::ComputerOverlay,
        ComputerUseCapabilityV1::FilesystemOverlaySupport,
        EvidenceState::Unknown,
        &[
            PrerequisiteCapability::ComputerGuestImage,
            PrerequisiteCapability::LinuxQemu,
            PrerequisiteCapability::LinuxVmResources,
        ],
    );
    push_component(
        snapshot,
        PrerequisiteCapability::ComputerNetworkGateway,
        ComputerUseCapabilityV1::FilteredNetworkGateway,
        EvidenceState::Unsupported,
        &[
            PrerequisiteCapability::ComputerGuestImage,
            PrerequisiteCapability::LinuxKvmDevice,
            PrerequisiteCapability::LinuxQemu,
            PrerequisiteCapability::LinuxVmResources,
        ],
    );
    push_component(
        snapshot,
        PrerequisiteCapability::ComputerGraphicsTransport,
        ComputerUseCapabilityV1::AuthenticatedGraphicsTransport,
        EvidenceState::Unsupported,
        &[
            PrerequisiteCapability::ComputerGuestImage,
            PrerequisiteCapability::LinuxKvmDevice,
            PrerequisiteCapability::LinuxQemu,
            PrerequisiteCapability::LinuxVmResources,
        ],
    );
    push_component(
        snapshot,
        PrerequisiteCapability::ComputerGuestProtocol,
        ComputerUseCapabilityV1::GuestProtocolCompatibility,
        EvidenceState::Unknown,
        &[PrerequisiteCapability::ComputerGuestImage],
    );
    push_component(
        snapshot,
        PrerequisiteCapability::ComputerEncryptionKeyService,
        ComputerUseCapabilityV1::PersonalEncryptionKeyService,
        EvidenceState::Unsupported,
        &[
            PrerequisiteCapability::ComputerGuestImage,
            PrerequisiteCapability::ComputerOverlay,
            PrerequisiteCapability::ComputerGuestProtocol,
        ],
    );
}

fn push_guest_image(
    snapshot: &mut MachineSnapshotV1,
    probe: super::guest_image::GuestImageCatalogProbe,
) {
    let mut evidence = base(snapshot, PrerequisiteCapability::ComputerGuestImage);
    evidence.state = probe.state;
    evidence.code = probe.code;
    evidence.origin = EvidenceOrigin::ApplicationInventory;
    evidence.observation = Some(ProbeObservationV1::ComputerUseCapability {
        capability: ComputerUseCapabilityV1::GuestImageCatalogIntegrity,
        readiness_probe_integrated: true,
    });
    push(snapshot, evidence, &[]);
}

fn push_component(
    snapshot: &mut MachineSnapshotV1,
    capability: PrerequisiteCapability,
    observed_capability: ComputerUseCapabilityV1,
    state: EvidenceState,
    dependencies: &[PrerequisiteCapability],
) {
    let mut evidence = base(snapshot, capability);
    evidence.state = state;
    evidence.code = EvidenceCode::ComputerUseReadinessProbeNotIntegrated;
    evidence.origin = EvidenceOrigin::ApplicationInventory;
    evidence.observation = Some(ProbeObservationV1::ComputerUseCapability {
        capability: observed_capability,
        readiness_probe_integrated: false,
    });
    push(snapshot, evidence, dependencies);
}

fn virtualization_evidence(
    snapshot: &MachineSnapshotV1,
    cpu: &Check<()>,
    nested: &Check<()>,
) -> ProbeEvidenceV1 {
    let mut evidence = base(snapshot, PrerequisiteCapability::LinuxVirtualization);
    let cpu_flag = match cpu {
        Check::Ready(()) => Some(true),
        Check::Missing => Some(false),
        _ => None,
    };
    let nested_hypervisor = match nested {
        Check::Ready(()) => Some(false),
        Check::Unsupported => Some(true),
        _ => None,
    };
    let (state, code) = match (cpu_flag, nested_hypervisor) {
        (_, Some(true)) => (
            EvidenceState::Unsupported,
            EvidenceCode::LinuxVirtualizationNestedUnsupported,
        ),
        (Some(true), Some(false)) => (
            EvidenceState::Ready,
            EvidenceCode::LinuxVirtualizationAvailable,
        ),
        (Some(false), Some(false)) => (
            EvidenceState::Missing,
            EvidenceCode::LinuxVirtualizationMissing,
        ),
        _ => (
            EvidenceState::Unknown,
            EvidenceCode::LinuxVirtualizationUnknown,
        ),
    };
    evidence.state = state;
    evidence.code = code;
    evidence.observation = Some(ProbeObservationV1::LinuxVirtualization {
        cpu_virtualization_flag: cpu_flag,
        nested_hypervisor,
    });
    evidence
}

fn push(
    snapshot: &mut MachineSnapshotV1,
    evidence: ProbeEvidenceV1,
    dependencies: &[PrerequisiteCapability],
) {
    snapshot.prerequisite_graph.push(PrerequisiteNodeV1 {
        id: evidence.prerequisite.clone(),
        state: evidence.state,
        depends_on: dependencies.iter().copied().map(prerequisite_id).collect(),
        remediation: RemediationEligibility::Unverified,
    });
    snapshot.probe_evidence.push(evidence);
}

fn prerequisite_id(capability: PrerequisiteCapability) -> PrerequisiteId {
    PrerequisiteId {
        domain: PrerequisiteDomain::Host,
        platform: HostKind::Linux,
        capability,
        subject: PrerequisiteSubject::None,
    }
}

fn base(snapshot: &MachineSnapshotV1, capability: PrerequisiteCapability) -> ProbeEvidenceV1 {
    ProbeEvidenceV1 {
        prerequisite: PrerequisiteId {
            domain: PrerequisiteDomain::Host,
            platform: HostKind::Linux,
            capability,
            subject: PrerequisiteSubject::None,
        },
        state: EvidenceState::Unknown,
        code: EvidenceCode::LinuxProbeMalformed,
        origin: EvidenceOrigin::HostRuntime,
        scan_generation: snapshot.scan_generation,
        source_generation: None,
        observed_at_unix_ms: snapshot.checked_at_unix_ms,
        observation: None,
    }
}

fn kvm_evidence(snapshot: &MachineSnapshotV1, result: &Check<()>) -> ProbeEvidenceV1 {
    let mut evidence = base(snapshot, PrerequisiteCapability::LinuxKvmDevice);
    let (state, code, accessible) = match result {
        Check::Ready(()) => (
            EvidenceState::Ready,
            EvidenceCode::LinuxKvmDeviceAccessible,
            true,
        ),
        Check::Missing => (
            EvidenceState::Missing,
            EvidenceCode::LinuxKvmDeviceMissing,
            false,
        ),
        Check::PermissionDenied => (
            EvidenceState::Failed,
            EvidenceCode::LinuxKvmDevicePermissionDenied,
            false,
        ),
        Check::Unsupported => (
            EvidenceState::Unsupported,
            EvidenceCode::LinuxProbeUnsupported,
            false,
        ),
        Check::Unknown => (
            EvidenceState::Unknown,
            EvidenceCode::LinuxKvmDeviceUnknown,
            false,
        ),
    };
    evidence.state = state;
    evidence.code = code;
    evidence.observation = Some(ProbeObservationV1::LinuxKvmDevice { accessible });
    evidence
}

fn qemu_evidence(
    snapshot: &MachineSnapshotV1,
    result: &Check<QemuCapabilities>,
    launcher_containment: Check<()>,
) -> ProbeEvidenceV1 {
    let mut evidence = base(snapshot, PrerequisiteCapability::LinuxQemu);
    let launcher_observed = match launcher_containment {
        Check::Ready(()) => Some(true),
        Check::Missing | Check::PermissionDenied => Some(false),
        Check::Unknown | Check::Unsupported => None,
    };
    let (
        state,
        code,
        version,
        kvm_acceleration,
        qemu_image_tool_available,
        qemu_launcher_contained,
    ) = match result {
        Check::Ready(qemu) => (
            EvidenceState::Ready,
            EvidenceCode::LinuxQemuDetected,
            Some(qemu.version.clone()),
            Some(qemu.kvm_acceleration),
            match qemu.qemu_image_tool {
                Check::Ready(()) => Some(true),
                Check::Missing | Check::PermissionDenied => Some(false),
                Check::Unknown | Check::Unsupported => None,
            },
            launcher_observed,
        ),
        Check::Missing => (
            EvidenceState::Missing,
            EvidenceCode::LinuxQemuMissing,
            None,
            None,
            None,
            launcher_observed,
        ),
        Check::PermissionDenied => (
            EvidenceState::Failed,
            EvidenceCode::LinuxQemuPermissionDenied,
            None,
            None,
            None,
            launcher_observed,
        ),
        Check::Unknown => (
            EvidenceState::Unknown,
            EvidenceCode::LinuxQemuUnknown,
            None,
            None,
            None,
            launcher_observed,
        ),
        Check::Unsupported => (
            EvidenceState::Unsupported,
            EvidenceCode::LinuxProbeUnsupported,
            None,
            None,
            None,
            launcher_observed,
        ),
    };
    evidence.state = state;
    evidence.code = code;
    evidence.observation = Some(ProbeObservationV1::LinuxQemu {
        version,
        kvm_acceleration,
        qemu_image_tool_available,
        qemu_launcher_contained,
    });
    evidence
}

fn resource_evidence(snapshot: &MachineSnapshotV1, capacity: ResourceCapacity) -> ProbeEvidenceV1 {
    let mut evidence = base(snapshot, PrerequisiteCapability::LinuxVmResources);
    evidence.state = capacity.state();
    evidence.code = capacity.code();
    evidence.observation = Some(ProbeObservationV1::LinuxVmResources {
        available_memory_bytes: capacity.available_memory_bytes,
        available_storage_bytes: capacity.available_storage_bytes,
        required_memory_bytes: crate::computer_use::MINIMUM_DISPOSABLE_VM_BUDGET_V1.memory_bytes,
        required_storage_bytes: crate::computer_use::MINIMUM_DISPOSABLE_VM_BUDGET_V1.disk_bytes,
        filesystem_family: capacity.filesystem_family,
    });
    evidence
}

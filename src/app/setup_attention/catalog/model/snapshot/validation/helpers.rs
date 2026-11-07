use super::super::super::evidence::{EvidenceCode, EvidenceOrigin};
use super::{
    EvidenceState, HostKind, ImageArchitectureV1, ImageProfileV1, PrerequisiteCapability,
    PrerequisiteDomain, PrerequisiteSubject, ProbeEvidenceV1, ProbeObservationV1,
};

mod computer_use;
mod qemu;
pub(super) use computer_use::computer_use_capability_evidence_is_consistent;
pub(super) use qemu::linux_qemu_evidence_is_consistent;

pub(crate) fn requires_macos_release(item: &ProbeEvidenceV1) -> bool {
    item.prerequisite.capability == PrerequisiteCapability::HostEdition
        && item.prerequisite.platform == HostKind::MacOs
        && item.state == EvidenceState::Ready
}

pub(super) fn observation_matches(item: &ProbeEvidenceV1) -> bool {
    use super::super::super::identifiers::PrerequisiteCapability as Capability;
    use super::super::super::identifiers::PrerequisiteDomain as Domain;
    matches!(
        (
            item.prerequisite.domain,
            item.prerequisite.capability,
            &item.observation
        ),
        (
            Domain::Host,
            Capability::HostArchitecture,
            Some(ProbeObservationV1::HostArchitecture { .. })
        ) | (
            Domain::Host,
            Capability::HostEdition,
            Some(ProbeObservationV1::MacOsVersion { .. })
        ) | (
            Domain::Host,
            Capability::LinuxDistribution,
            Some(ProbeObservationV1::LinuxDistribution { .. })
        ) | (
            Domain::Host,
            Capability::HostEdition,
            Some(ProbeObservationV1::WindowsHost { .. })
        ) | (
            Domain::Docker,
            Capability::DockerEndpoint,
            Some(ProbeObservationV1::DockerEngineIdentity { .. })
        ) | (
            Domain::Host,
            Capability::LinuxRootlessRecipe,
            Some(ProbeObservationV1::LinuxRootlessRecipe { .. })
        ) | (
            Domain::Host,
            Capability::LinuxKernel,
            Some(ProbeObservationV1::LinuxKernel { .. })
        ) | (
            Domain::Host,
            Capability::LinuxUserNamespaces,
            Some(ProbeObservationV1::LinuxUserNamespaces { .. })
        ) | (
            Domain::Host,
            Capability::LinuxSubuid | Capability::LinuxSubgid,
            Some(ProbeObservationV1::LinuxSubordinateIds { .. })
        ) | (
            Domain::Host,
            Capability::LinuxRootlessHelpers,
            Some(ProbeObservationV1::LinuxRootlessHelpers { .. })
        ) | (
            Domain::Host,
            Capability::LinuxCgroups,
            Some(ProbeObservationV1::LinuxCgroups { .. })
        ) | (
            Domain::Host,
            Capability::LinuxServiceManager,
            Some(ProbeObservationV1::LinuxServiceManager { .. })
        ) | (
            Domain::Host,
            Capability::LinuxPackageManager,
            Some(ProbeObservationV1::LinuxPackageManager { .. })
        ) | (
            Domain::Host,
            Capability::LinuxKvmDevice,
            Some(ProbeObservationV1::LinuxKvmDevice { .. })
        ) | (
            Domain::Host,
            Capability::LinuxQemu,
            Some(ProbeObservationV1::LinuxQemu { .. })
        ) | (
            Domain::Host,
            Capability::LinuxVmResources,
            Some(ProbeObservationV1::LinuxVmResources { .. })
        ) | (
            Domain::Host,
            Capability::ComputerGuestImage
                | Capability::ComputerOverlay
                | Capability::ComputerNetworkGateway
                | Capability::ComputerGraphicsTransport
                | Capability::ComputerEncryptionKeyService
                | Capability::ComputerGuestProtocol,
            Some(ProbeObservationV1::ComputerUseCapability { .. })
        ) | (
            Domain::Host,
            Capability::Virtualization,
            Some(ProbeObservationV1::WindowsVirtualization { .. })
        ) | (
            Domain::Host,
            Capability::LinuxVirtualization,
            Some(ProbeObservationV1::LinuxVirtualization { .. })
        ) | (
            Domain::Host,
            Capability::WindowsWslFeature | Capability::WindowsVmPlatformFeature,
            Some(ProbeObservationV1::WindowsOptionalFeature { .. })
        ) | (
            Domain::Host,
            Capability::WindowsWslKernel,
            Some(ProbeObservationV1::WindowsWslKernel { .. })
        ) | (
            Domain::Host,
            Capability::WindowsReboot,
            Some(ProbeObservationV1::WindowsRebootPending { .. })
        ) | (
            Domain::Host,
            Capability::WindowsWslPolicy,
            Some(ProbeObservationV1::WindowsWslPolicy { .. })
        ) | (
            Domain::Docker,
            Capability::DockerRootlessSmoke,
            Some(ProbeObservationV1::DockerRootlessSmoke { .. })
        ) | (_, _, None)
    )
}

pub(super) fn linux_kvm_evidence_is_consistent(item: &ProbeEvidenceV1) -> bool {
    if item.prerequisite.domain != PrerequisiteDomain::Host
        || item.prerequisite.platform != HostKind::Linux
        || item.prerequisite.subject != PrerequisiteSubject::None
        || item.origin != EvidenceOrigin::HostRuntime
        || item.source_generation.is_some()
    {
        return false;
    }
    let Some(ProbeObservationV1::LinuxKvmDevice { accessible }) = item.observation.as_ref() else {
        return false;
    };
    matches!(
        (item.state, item.code, *accessible),
        (
            EvidenceState::Ready,
            EvidenceCode::LinuxKvmDeviceAccessible,
            true
        ) | (
            EvidenceState::Missing,
            EvidenceCode::LinuxKvmDeviceMissing,
            false
        ) | (
            EvidenceState::Failed,
            EvidenceCode::LinuxKvmDevicePermissionDenied,
            false
        ) | (
            EvidenceState::Unsupported,
            EvidenceCode::LinuxKvmNestedUnsupported,
            false
        ) | (
            EvidenceState::Unsupported,
            EvidenceCode::LinuxProbeUnsupported,
            false
        ) | (
            EvidenceState::Unknown,
            EvidenceCode::LinuxKvmDeviceUnknown,
            false
        )
    )
}

pub(super) fn linux_virtualization_evidence_is_consistent(item: &ProbeEvidenceV1) -> bool {
    if item.prerequisite.domain != PrerequisiteDomain::Host
        || item.prerequisite.platform != HostKind::Linux
        || item.prerequisite.subject != PrerequisiteSubject::None
        || item.origin != EvidenceOrigin::HostRuntime
        || item.source_generation.is_some()
    {
        return false;
    }
    let Some(ProbeObservationV1::LinuxVirtualization {
        cpu_virtualization_flag,
        nested_hypervisor,
    }) = item.observation.as_ref()
    else {
        return false;
    };
    match (
        item.state,
        item.code,
        cpu_virtualization_flag,
        nested_hypervisor,
    ) {
        (
            EvidenceState::Ready,
            EvidenceCode::LinuxVirtualizationAvailable,
            Some(true),
            Some(false),
        )
        | (
            EvidenceState::Missing,
            EvidenceCode::LinuxVirtualizationMissing,
            Some(false),
            Some(false),
        )
        | (
            EvidenceState::Unsupported,
            EvidenceCode::LinuxVirtualizationNestedUnsupported,
            _,
            Some(true),
        ) => true,
        (EvidenceState::Unknown, EvidenceCode::LinuxVirtualizationUnknown, cpu, nested) => {
            !(*cpu == Some(true) && *nested == Some(false))
                && !(*cpu == Some(false) && *nested == Some(false))
                && *nested != Some(true)
        }
        _ => false,
    }
}

pub(super) fn linux_vm_resources_evidence_is_consistent(item: &ProbeEvidenceV1) -> bool {
    use crate::computer_use::MINIMUM_DISPOSABLE_VM_BUDGET_V1 as budget;
    if item.prerequisite.domain != PrerequisiteDomain::Host
        || item.prerequisite.platform != HostKind::Linux
        || item.prerequisite.subject != PrerequisiteSubject::None
        || item.origin != EvidenceOrigin::HostRuntime
        || item.source_generation.is_some()
    {
        return false;
    }
    let Some(ProbeObservationV1::LinuxVmResources {
        available_memory_bytes,
        available_storage_bytes,
        required_memory_bytes,
        required_storage_bytes,
        filesystem_family,
        ..
    }) = item.observation.as_ref()
    else {
        return false;
    };
    let short = available_memory_bytes.is_some_and(|value| value < budget.memory_bytes)
        || available_storage_bytes.is_some_and(|value| value < budget.disk_bytes);
    let ready = available_memory_bytes.is_some_and(|value| value >= budget.memory_bytes)
        && available_storage_bytes.is_some_and(|value| value >= budget.disk_bytes);
    *required_memory_bytes == budget.memory_bytes
        && *required_storage_bytes == budget.disk_bytes
        && (filesystem_family.is_none() || available_storage_bytes.is_some())
        && match item.state {
            EvidenceState::Missing => {
                short && item.code == EvidenceCode::LinuxVmResourcesInsufficient
            }
            EvidenceState::Ready => ready && item.code == EvidenceCode::LinuxVmResourcesAvailable,
            EvidenceState::Unknown => {
                !short && !ready && item.code == EvidenceCode::LinuxVmResourcesUnknown
            }
            _ => false,
        }
}

pub(crate) fn valid_macos_version(version: &str) -> bool {
    version.len() <= 32
        && !version.is_empty()
        && version
            .split('.')
            .all(|part| !part.is_empty() && part.bytes().all(|byte| byte.is_ascii_digit()))
}

pub(crate) fn rootless_smoke_subject_matches(
    subject: &PrerequisiteSubject,
    image_platform: &str,
) -> bool {
    matches!(
        (subject, image_platform),
        (
            PrerequisiteSubject::Image {
                profile: ImageProfileV1::General,
                architecture: ImageArchitectureV1::Amd64,
            },
            "linux/amd64",
        ) | (
            PrerequisiteSubject::Image {
                profile: ImageProfileV1::General,
                architecture: ImageArchitectureV1::Arm64,
            },
            "linux/arm64",
        )
    )
}

pub(super) fn valid_sha256(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

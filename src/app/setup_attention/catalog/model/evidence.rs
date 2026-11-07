use serde::{Deserialize, Serialize};

use super::identifiers::PrerequisiteId;

mod observations;
pub use observations::{ComputerUseCapabilityV1, LinuxVmStorageFilesystemV1, ProbeObservationV1};

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EvidenceState {
    Ready,
    Missing,
    Unknown,
    Stale,
    Failed,
    Unsupported,
    Blocked,
    NotApplicable,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EvidenceOrigin {
    HostRuntime,
    ApplicationInventory,
    ProviderCli,
    DockerDiscovery,
    ImageInventory,
    ApprovedImageSmokeTest,
    GitCli,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EvidenceCode {
    RuntimeTargetDetected,
    HostEditionNotProbed,
    MacOsReleaseDetected,
    HostCapabilityNotProbed,
    WindowsHostDetected,
    WindowsHostUnsupported,
    WindowsFeatureEnabled,
    WindowsFeatureDisabled,
    WindowsFeaturePendingReboot,
    WindowsWslKernelDetected,
    WindowsWslKernelUnknown,
    WindowsRebootPending,
    WindowsNoRebootPending,
    WindowsWslPolicyBlocked,
    WindowsWslPolicyAllowed,
    WindowsProbeInaccessible,
    WindowsProbeMalformed,
    LinuxDistributionDetected,
    LinuxHostCheckReady,
    LinuxHostCheckMissing,
    LinuxProbeInaccessible,
    LinuxProbeMalformed,
    LinuxProbeUnsupported,
    LinuxRootlessRecipeCandidate,
    LinuxRootlessRecipeUnverified,
    LinuxUidMappingsConfigured,
    LinuxUidMappingsMissing,
    LinuxServiceManagerDetected,
    LinuxServiceManagerUnknown,
    LinuxPackageManagerDetected,
    LinuxPackageManagerMissing,
    LinuxKvmDeviceAccessible,
    LinuxKvmDeviceMissing,
    LinuxKvmDevicePermissionDenied,
    LinuxKvmDeviceUnknown,
    LinuxKvmNestedUnsupported,
    LinuxVirtualizationAvailable,
    LinuxVirtualizationMissing,
    LinuxVirtualizationNestedUnsupported,
    LinuxVirtualizationUnknown,
    LinuxQemuDetected,
    LinuxQemuMissing,
    LinuxQemuPermissionDenied,
    LinuxQemuUnknown,
    LinuxVmResourcesAvailable,
    LinuxVmResourcesInsufficient,
    LinuxVmResourcesUnknown,
    ComputerUseReadinessProbeNotIntegrated,
    ComputerGuestImageNotPromoted,
    ComputerGuestImageCatalogUnverified,
    ComputerGuestImageCatalogVerified,
    ArchitectureDetected,
    ProviderInstalled,
    ProviderPathInvalid,
    ProviderMissing,
    ProviderProbeFailed,
    StaleProbeGeneration,
    ProviderVersionDetected,
    ProviderVersionUnsupported,
    ProviderVersionUnknown,
    AuthenticationVerified,
    AuthenticationRequired,
    AuthenticationUnknown,
    ModelCatalogVerified,
    ModelCatalogStale,
    ModelCatalogUnavailable,
    ContextOnlyVerified,
    ContextOnlyUnavailable,
    ContextOnlyUnknown,
    DockerReady,
    DockerMissing,
    DockerStopped,
    DockerPermissionDenied,
    DockerRootfulRejected,
    DockerWindowsContainers,
    DockerEndpointUntrusted,
    DockerCapabilityUnknown,
    DockerCapacityUnavailable,
    DockerDiskUnavailable,
    DockerLimitsUnavailable,
    DockerSeccompUnavailable,
    DockerApiUnsupported,
    DockerOsUnsupported,
    DockerArchitectureUnsupported,
    DockerWorkspaceUnavailable,
    DockerProfileUnsupported,
    DockerImageUncheckable,
    DockerImageDigestMismatch,
    DockerImageArchitectureMismatch,
    DockerDesktopVmDetected,
    DockerDesktopTrustVerified,
    DockerDesktopTrustUnverified,
    DockerCgroupsSupported,
    DockerCgroupsUnavailable,
    ApprovedImageSmokeSucceeded,
    ImagePresent,
    ImageMissing,
    ImageUncheckable,
    ParentPrerequisiteUnavailable,
    GitAvailable,
    GitMissing,
    GitProbeFailed,
    GitProbeNotRun,
    GitIdentityConfigured,
    GitIdentityMissing,
    GitIdentityProbeFailed,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct GitProbeV1 {
    pub executable_state: EvidenceState,
    pub executable_code: EvidenceCode,
    pub identity_state: EvidenceState,
    pub identity_code: EvidenceCode,
}

impl GitProbeV1 {
    #[cfg(test)]
    pub const fn not_run() -> Self {
        Self {
            executable_state: EvidenceState::Unknown,
            executable_code: EvidenceCode::GitProbeNotRun,
            identity_state: EvidenceState::Blocked,
            identity_code: EvidenceCode::ParentPrerequisiteUnavailable,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct ProbeEvidenceV1 {
    pub prerequisite: PrerequisiteId,
    pub state: EvidenceState,
    pub code: EvidenceCode,
    pub origin: EvidenceOrigin,
    pub scan_generation: u64,
    pub source_generation: Option<u64>,
    pub observed_at_unix_ms: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub observation: Option<ProbeObservationV1>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RemediationEligibility {
    Eligible,
    BlockedByPrerequisite,
    Unverified,
    NotRequired,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct PrerequisiteNodeV1 {
    pub id: PrerequisiteId,
    pub state: EvidenceState,
    pub depends_on: Vec<PrerequisiteId>,
    pub remediation: RemediationEligibility,
}

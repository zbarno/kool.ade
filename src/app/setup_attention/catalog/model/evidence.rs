use serde::{Deserialize, Serialize};

use super::identifiers::{
    ImageArchitectureV1, LinuxDistributionV1, LinuxDistributionVersionV1, LinuxKernelVersionV1,
    LinuxPackageManagerV1, LinuxRootlessRecipeV1, PrerequisiteId,
};

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
    HostCapabilityNotProbed,
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
    ArchitectureDetected,
    ProviderInstalled,
    ProviderMissing,
    ProviderProbeFailed,
    StaleProbeGeneration,
    ProviderVersionDetected,
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

/// Closed, privacy-safe typed values that bind host capability claims to their
/// originating evidence. Raw host command output is never retained.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub enum ProbeObservationV1 {
    HostArchitecture {
        observed: ImageArchitectureV1,
        supported_runtime_architectures: Vec<ImageArchitectureV1>,
    },
    LinuxDistribution {
        distribution: Option<LinuxDistributionV1>,
        version: Option<LinuxDistributionVersionV1>,
    },
    DockerEngineIdentity {
        fingerprint: String,
    },
    LinuxRootlessRecipe {
        candidate: Option<LinuxRootlessRecipeV1>,
        distribution_state: EvidenceState,
    },
    LinuxKernel {
        version: LinuxKernelVersionV1,
    },
    LinuxUserNamespaces {
        max_user_namespaces: Option<u32>,
        unprivileged_clone_allowed: Option<bool>,
        apparmor_userns_restricted: Option<bool>,
    },
    LinuxSubordinateIds {
        configured: bool,
    },
    LinuxRootlessHelpers {
        newuidmap: bool,
        newgidmap: bool,
        rootlesskit: bool,
        dockerd_rootless: bool,
        setuptool: bool,
        slirp4netns: Option<bool>,
    },
    LinuxCgroups {
        cgroup_v2: bool,
        cpu_delegated: bool,
        memory_delegated: bool,
        pids_delegated: bool,
    },
    LinuxServiceManager {
        runtime_directory_ready: bool,
        private_socket_ready: bool,
        bus_socket_ready: bool,
    },
    LinuxPackageManager {
        expected: Option<LinuxPackageManagerV1>,
        found: Option<LinuxPackageManagerV1>,
    },
    DockerRootlessSmoke {
        endpoint_fingerprint: String,
        daemon_fingerprint: String,
        daemon_version: String,
        image_reference: String,
        image_digest: String,
        image_platform: String,
        completed_at_unix_ms: u64,
    },
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

use serde::{Deserialize, Serialize};

use super::super::identifiers::{
    ImageArchitectureV1, LinuxDistributionV1, LinuxDistributionVersionV1, LinuxKernelVersionV1,
    LinuxPackageManagerV1, LinuxRootlessRecipeV1, WindowsEditionV1, WindowsKernelVersionV1,
    WindowsProductV1,
};
use super::EvidenceState;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LinuxVmStorageFilesystemV1 {
    Ext,
    Xfs,
    Btrfs,
    Other,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ComputerUseCapabilityV1 {
    GuestImageCatalogIntegrity,
    FilesystemOverlaySupport,
    FilteredNetworkGateway,
    AuthenticatedGraphicsTransport,
    PersonalEncryptionKeyService,
    GuestProtocolCompatibility,
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
    MacOsVersion {
        version: String,
    },
    WindowsHost {
        product: WindowsProductV1,
        edition: WindowsEditionV1,
        build_number: u32,
        architecture: ImageArchitectureV1,
    },
    WindowsOptionalFeature {
        enabled: bool,
        pending_reboot: bool,
    },
    WindowsWslKernel {
        version: Option<WindowsKernelVersionV1>,
    },
    WindowsRebootPending {
        pending: bool,
    },
    WindowsWslPolicy {
        allowed: Option<bool>,
    },
    WindowsVirtualization {
        enabled: bool,
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
    LinuxKvmDevice {
        accessible: bool,
    },
    LinuxVirtualization {
        cpu_virtualization_flag: Option<bool>,
        nested_hypervisor: Option<bool>,
    },
    LinuxQemu {
        version: Option<String>,
        kvm_acceleration: Option<bool>,
        qemu_image_tool_available: Option<bool>,
        qemu_launcher_contained: Option<bool>,
    },
    LinuxVmResources {
        available_memory_bytes: Option<u64>,
        available_storage_bytes: Option<u64>,
        required_memory_bytes: u64,
        required_storage_bytes: u64,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        filesystem_family: Option<LinuxVmStorageFilesystemV1>,
    },
    ComputerUseCapability {
        capability: ComputerUseCapabilityV1,
        readiness_probe_integrated: bool,
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

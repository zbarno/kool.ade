use serde::{Deserialize, Serialize};

use super::engine::EngineSnapshot;

pub const MIN_API_VERSION: (u16, u16) = (1, 41);
pub const MAX_API_VERSION: (u16, u16) = (1, 53);

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PrerequisiteCode {
    CapabilitiesIncompatible,
    DockerMissing,
    DaemonStopped,
    PermissionDenied,
    UntrustedEndpoint,
    RootfulLinux,
    WindowsContainers,
    CgroupsUnavailable,
    InsufficientDisk,
    LimitsUnavailable,
    SeccompUnavailable,
    UnsupportedApi,
    UnsupportedOs,
    UnsupportedArchitecture,
    ImageUnavailable,
    ImageDigestMismatch,
    ImageArchitectureMismatch,
    WorkspaceUnavailable,
    ProfileUnsupported,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PrerequisiteFailure {
    pub code: PrerequisiteCode,
    pub message: String,
    pub retryable: bool,
}

impl std::fmt::Display for PrerequisiteFailure {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for PrerequisiteFailure {}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct EngineReadiness {
    pub ready: bool,
    /// The native Desktop peer or named pipe was authenticated and its Engine
    /// passed the Linux-container and security checks. Storage capacity is a
    /// separate readiness gate.
    #[serde(default)]
    pub desktop_transport_verified: bool,
    /// The dedicated guest VM capacity probe returned a current free-space sample.
    #[serde(default)]
    pub desktop_vm_capacity_verified: bool,
    /// Hash of the daemon ID; never contains the daemon's raw identity.
    pub daemon_identity_fingerprint: Option<String>,
    pub docker_desktop: bool,
    pub daemon_version: Option<String>,
    pub api_version: Option<String>,
    pub architecture: Option<String>,
    pub rootless: Option<bool>,
    pub linux_containers: Option<bool>,
    pub seccomp_enabled: Option<bool>,
    pub memory_limit: Option<bool>,
    pub cpu_quota: Option<bool>,
    pub pids_limit: Option<bool>,
    pub cgroups_supported: Option<bool>,
    /// The application-owned staging directory is accessible and meets the
    /// configured minimum free-space requirement.
    pub workspace_staging_available: bool,
    pub disk_available_bytes: Option<u64>,
    pub available_images: Vec<String>,
    pub failure: Option<PrerequisiteFailure>,
}

pub fn validate_snapshot(snapshot: &EngineSnapshot) -> Result<(), PrerequisiteFailure> {
    validate_snapshot_engine(snapshot)?;
    validate_snapshot_storage(snapshot)
}

/// Validate daemon identity, Linux-container compatibility, and isolation
/// capabilities separately from storage telemetry. Desktop transport trust
/// uses this result, while execution readiness still requires storage below.
pub(crate) fn validate_snapshot_engine(
    snapshot: &EngineSnapshot,
) -> Result<(), PrerequisiteFailure> {
    let fail = |code, message: &str, retryable| {
        Err(PrerequisiteFailure {
            code,
            message: message.into(),
            retryable,
        })
    };
    let api = parse_api_version(&snapshot.api_version).ok_or_else(|| PrerequisiteFailure {
        code: PrerequisiteCode::UnsupportedApi,
        message: "Docker returned an invalid Engine API version".into(),
        retryable: false,
    })?;
    let minimum_api =
        parse_api_version(&snapshot.minimum_api_version).ok_or_else(|| PrerequisiteFailure {
            code: PrerequisiteCode::UnsupportedApi,
            message: "Docker returned an invalid minimum Engine API version".into(),
            retryable: false,
        })?;
    if api < MIN_API_VERSION || minimum_api > MAX_API_VERSION || minimum_api > api {
        return fail(
            PrerequisiteCode::UnsupportedApi,
            "Docker Engine API does not overlap the supported client range (1.41 through 1.53)",
            false,
        );
    }
    if snapshot.os_type.eq_ignore_ascii_case("windows") {
        return fail(
            PrerequisiteCode::WindowsContainers,
            "Docker is using Windows containers; switch Docker Desktop to Linux containers",
            false,
        );
    }
    if !snapshot.os_type.eq_ignore_ascii_case("linux") {
        return fail(
            PrerequisiteCode::UnsupportedOs,
            "Docker Engine must provide Linux containers",
            false,
        );
    }
    if !matches!(snapshot.architecture.as_str(), "amd64" | "arm64") {
        return fail(
            PrerequisiteCode::UnsupportedArchitecture,
            "Docker Engine architecture is unsupported; use linux/amd64 or linux/arm64",
            false,
        );
    }
    if snapshot.native_linux && !snapshot.docker_desktop && !snapshot.rootless {
        return fail(
            PrerequisiteCode::RootfulLinux,
            "Native Linux requires rootless Docker Engine; membership in the docker group is not sufficient",
            false,
        );
    }
    match (
        snapshot.cgroup_version.as_deref(),
        snapshot.cgroup_driver.as_deref(),
    ) {
        (Some("1" | "2"), Some("systemd" | "cgroupfs")) => {}
        (Some(_), Some(_)) => {
            return fail(
                PrerequisiteCode::CgroupsUnavailable,
                "Docker reports an unsupported cgroup version or driver",
                false,
            );
        }
        _ => {
            return fail(
                PrerequisiteCode::CapabilitiesIncompatible,
                "Docker cgroup version and driver could not be verified",
                false,
            );
        }
    }
    if !snapshot.memory_limit || !snapshot.cpu_quota || !snapshot.pids_limit {
        return fail(
            PrerequisiteCode::LimitsUnavailable,
            "Docker does not report support for required memory, CPU quota, and PID limits",
            false,
        );
    }
    if !snapshot.seccomp_enabled {
        return fail(
            PrerequisiteCode::SeccompUnavailable,
            "Docker does not report an active seccomp profile",
            false,
        );
    }
    if snapshot.security_options.iter().any(|option| {
        let option = option.to_ascii_lowercase();
        option.contains("unconfined")
    }) {
        return fail(
            PrerequisiteCode::SeccompUnavailable,
            "Docker reports an unconfined seccomp or AppArmor policy",
            false,
        );
    }
    Ok(())
}

fn validate_snapshot_storage(snapshot: &EngineSnapshot) -> Result<(), PrerequisiteFailure> {
    let fail = |code, message: &str, retryable| {
        Err(PrerequisiteFailure {
            code,
            message: message.into(),
            retryable,
        })
    };
    if !snapshot.docker_storage_accessible {
        return fail(
            PrerequisiteCode::InsufficientDisk,
            if snapshot.docker_desktop {
                "Docker Desktop's VM storage capacity could not be verified from this host; Docker execution is unavailable until supported Desktop storage telemetry is present"
            } else {
                "Free space in Docker's image storage could not be verified"
            },
            false,
        );
    }
    if snapshot
        .disk_available_bytes
        .is_none_or(|bytes| bytes < snapshot.minimum_disk_bytes)
    {
        return fail(
            PrerequisiteCode::InsufficientDisk,
            "Docker workspace staging has less free space than the configured minimum",
            true,
        );
    }
    Ok(())
}

pub fn parse_api_version(value: &str) -> Option<(u16, u16)> {
    let (major, minor) = value.split_once('.')?;
    if major.is_empty()
        || minor.is_empty()
        || !major.bytes().all(|b| b.is_ascii_digit())
        || !minor.bytes().all(|b| b.is_ascii_digit())
    {
        return None;
    }
    Some((major.parse().ok()?, minor.parse().ok()?))
}

pub fn parse_architecture(value: &str) -> Option<&'static str> {
    match value {
        "amd64" | "x86_64" => Some("amd64"),
        "arm64" | "aarch64" => Some("arm64"),
        _ => None,
    }
}

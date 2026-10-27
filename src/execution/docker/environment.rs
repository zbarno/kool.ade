//! Cached, read-only local Docker readiness for Settings and execution routing.

use std::{
    sync::{Mutex, OnceLock},
    time::{Duration, Instant},
};

use serde::{Deserialize, Serialize};

use super::{
    discovery::{EngineReadiness, PrerequisiteCode, PrerequisiteFailure},
    engine::{DockerEndpoint, HostPlatform},
};
use probe::probe;
use rootless_selection::persist_rootless_selection;

const CACHE_TTL: Duration = Duration::from_secs(15);
static CACHE: OnceLock<Mutex<Option<(Instant, DockerEnvironmentStatus)>>> = OnceLock::new();

fn process_endpoint(platform: HostPlatform) -> Result<DockerEndpoint, PrerequisiteFailure> {
    #[cfg(test)]
    {
        DockerEndpoint::from_process_environment_with_test_support(platform)
    }
    #[cfg(not(test))]
    {
        DockerEndpoint::from_process_environment(platform)
    }
}

mod identity;
mod probe;
mod rootless_selection;
mod staging;
pub use probe::run_smoke_test;
pub(crate) use staging::app_staging_root;

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DockerEnvironmentState {
    NotInstalled,
    NotRunning,
    PermissionDenied,
    RootfulLinuxRejected,
    WindowsContainersRequired,
    CgroupsUnavailable,
    InsufficientDisk,
    LimitsUnavailable,
    SeccompUnavailable,
    UnsupportedApi,
    UnsupportedOs,
    UnsupportedArchitecture,
    WorkspaceUnavailable,
    ProfileUnsupported,
    DesktopVmRequired,
    MissingApprovedImage,
    ApprovedImageUncheckable,
    ApprovedImageDigestMismatch,
    ApprovedImageArchitectureMismatch,
    CapabilitiesIncompatible,
    UntrustedEndpoint,
    Ready,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct DockerEnvironmentStatus {
    pub state: DockerEnvironmentState,
    /// Docker and host capabilities are usable; an image can be pulled from
    /// the bundled lock before an execution starts.
    pub engine_ready: bool,
    pub message: String,
    pub desktop_vm: bool,
    /// The Desktop peer or pipe and its Linux Engine passed verification.
    /// Desktop VM capacity is checked separately by `engine_ready`.
    #[serde(default)]
    pub desktop_transport_verified: bool,
    /// A dedicated Linux VM probe measured current free space.
    #[serde(default)]
    pub desktop_vm_capacity_verified: bool,
    /// The application-owned task staging directory is accessible and has
    /// enough free space for the configured minimum.
    #[serde(default)]
    pub workspace_staging_available: bool,
    pub daemon_version: Option<String>,
    pub architecture: Option<String>,
    pub rootless: Option<bool>,
    pub cpu_limits: Option<bool>,
    pub memory_limits: Option<bool>,
    pub pids_limit: Option<bool>,
    #[serde(default)]
    pub cgroups_supported: Option<bool>,
    pub available_images: Vec<String>,
    pub disk_available_bytes: Option<u64>,
    /// Ephemeral fingerprints fence observations without storing endpoint paths or daemon IDs.
    #[serde(skip)]
    pub(crate) endpoint_fingerprint: Option<String>,
    #[serde(skip)]
    pub(crate) daemon_identity_fingerprint: Option<String>,
}

impl DockerEnvironmentStatus {
    pub fn detect() -> Self {
        let current_endpoint_fingerprint = Self::current_endpoint_fingerprint();
        let cache = CACHE.get_or_init(|| Mutex::new(None));
        if let Ok(cache) = cache.lock()
            && let Some((checked_at, status)) = &*cache
            && checked_at.elapsed() < CACHE_TTL
            && status.endpoint_fingerprint == current_endpoint_fingerprint
        {
            let mut status = status.clone();
            persist_rootless_selection(&mut status);
            return status;
        }
        let mut status = probe();
        persist_rootless_selection(&mut status);
        if let Ok(mut cache) = cache.lock() {
            *cache = Some((Instant::now(), status.clone()));
        }
        status
    }

    pub fn refresh() -> Self {
        Self::invalidate();
        Self::detect()
    }

    pub fn invalidate() {
        if let Some(cache) = CACHE.get()
            && let Ok(mut cache) = cache.lock()
        {
            *cache = None;
        }
    }

    pub fn from_failure(failure: PrerequisiteFailure, desktop_host: bool) -> Self {
        let cgroups_supported =
            (failure.code == PrerequisiteCode::CgroupsUnavailable).then_some(false);
        let state = match failure.code {
            PrerequisiteCode::DockerMissing if desktop_host => {
                DockerEnvironmentState::DesktopVmRequired
            }
            PrerequisiteCode::DockerMissing => DockerEnvironmentState::NotInstalled,
            PrerequisiteCode::DaemonStopped => DockerEnvironmentState::NotRunning,
            PrerequisiteCode::PermissionDenied => DockerEnvironmentState::PermissionDenied,
            PrerequisiteCode::RootfulLinux => DockerEnvironmentState::RootfulLinuxRejected,
            PrerequisiteCode::UntrustedEndpoint => DockerEnvironmentState::UntrustedEndpoint,
            PrerequisiteCode::WindowsContainers => {
                DockerEnvironmentState::WindowsContainersRequired
            }
            PrerequisiteCode::CgroupsUnavailable => DockerEnvironmentState::CgroupsUnavailable,
            PrerequisiteCode::InsufficientDisk => DockerEnvironmentState::InsufficientDisk,
            PrerequisiteCode::LimitsUnavailable => DockerEnvironmentState::LimitsUnavailable,
            PrerequisiteCode::SeccompUnavailable => DockerEnvironmentState::SeccompUnavailable,
            PrerequisiteCode::UnsupportedApi => DockerEnvironmentState::UnsupportedApi,
            PrerequisiteCode::UnsupportedOs => DockerEnvironmentState::UnsupportedOs,
            PrerequisiteCode::UnsupportedArchitecture => {
                DockerEnvironmentState::UnsupportedArchitecture
            }
            PrerequisiteCode::WorkspaceUnavailable => DockerEnvironmentState::WorkspaceUnavailable,
            PrerequisiteCode::ProfileUnsupported => DockerEnvironmentState::ProfileUnsupported,
            PrerequisiteCode::ImageUnavailable => DockerEnvironmentState::ApprovedImageUncheckable,
            PrerequisiteCode::ImageDigestMismatch => {
                DockerEnvironmentState::ApprovedImageDigestMismatch
            }
            PrerequisiteCode::ImageArchitectureMismatch => {
                DockerEnvironmentState::ApprovedImageArchitectureMismatch
            }
            _ => DockerEnvironmentState::CapabilitiesIncompatible,
        };
        Self {
            state,
            engine_ready: false,
            message: failure.message,
            desktop_vm: false,
            desktop_transport_verified: false,
            desktop_vm_capacity_verified: false,
            workspace_staging_available: false,
            daemon_version: None,
            architecture: None,
            rootless: None,
            cpu_limits: None,
            memory_limits: None,
            pids_limit: None,
            cgroups_supported,
            available_images: Vec::new(),
            disk_available_bytes: None,
            endpoint_fingerprint: None,
            daemon_identity_fingerprint: None,
        }
    }

    pub fn from_readiness(readiness: EngineReadiness) -> Self {
        let state = if readiness.ready && readiness.available_images.is_empty() {
            DockerEnvironmentState::MissingApprovedImage
        } else if readiness.ready {
            DockerEnvironmentState::Ready
        } else {
            readiness
                .failure
                .clone()
                .map(|failure| Self::from_failure(failure, readiness.docker_desktop).state)
                .unwrap_or(DockerEnvironmentState::CapabilitiesIncompatible)
        };
        let message = readiness
            .failure
            .as_ref()
            .map(|failure| failure.message.clone())
            .unwrap_or_else(|| {
                if readiness.available_images.is_empty() {
                    "Docker is available, but no approved runtime image digest is installed".into()
                } else {
                    "Docker Engine and approved runtime images are ready".into()
                }
            });
        Self {
            state,
            engine_ready: readiness.ready,
            message,
            desktop_vm: readiness.docker_desktop,
            desktop_transport_verified: readiness.desktop_transport_verified
                && readiness.docker_desktop
                && readiness.linux_containers == Some(true),
            desktop_vm_capacity_verified: readiness.desktop_vm_capacity_verified
                && readiness.docker_desktop
                && readiness.desktop_transport_verified,
            workspace_staging_available: readiness.workspace_staging_available,
            daemon_version: readiness.daemon_version,
            architecture: readiness.architecture,
            rootless: readiness.rootless,
            cpu_limits: readiness.cpu_quota,
            memory_limits: readiness.memory_limit,
            pids_limit: readiness.pids_limit,
            cgroups_supported: readiness.cgroups_supported,
            available_images: readiness.available_images,
            disk_available_bytes: readiness.disk_available_bytes,
            endpoint_fingerprint: None,
            daemon_identity_fingerprint: readiness.daemon_identity_fingerprint,
        }
    }

    pub fn ready(&self) -> bool {
        self.engine_ready
    }

    pub fn local_execution_ready(&self) -> bool {
        (cfg!(target_os = "linux")
            && self.engine_ready
            && !self.desktop_vm
            && self.rootless == Some(true)
            && self.workspace_staging_available)
            || (cfg!(any(target_os = "macos", target_os = "windows"))
                && self.engine_ready
                && self.desktop_vm
                && self.desktop_transport_verified
                && self.desktop_vm_capacity_verified
                && self.workspace_staging_available)
    }
}

#[cfg(test)]
#[path = "environment/tests.rs"]
mod tests;

//! Cached, read-only local Docker readiness for Settings and execution routing.

use std::{
    sync::{Mutex, OnceLock},
    time::{Duration, Instant},
};

use serde::{Deserialize, Serialize};

use super::{
    discovery::{EngineReadiness, PrerequisiteCode, PrerequisiteFailure},
    engine::DockerEndpoint,
};
use probe::{endpoint_fingerprint, host_platform, probe};

const CACHE_TTL: Duration = Duration::from_secs(15);
static CACHE: OnceLock<Mutex<Option<(Instant, DockerEnvironmentStatus)>>> = OnceLock::new();

mod probe;
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
        let cache = CACHE.get_or_init(|| Mutex::new(None));
        if let Ok(cache) = cache.lock()
            && let Some((checked_at, status)) = &*cache
            && checked_at.elapsed() < CACHE_TTL
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
        cfg!(target_os = "linux")
            && self.engine_ready
            && !self.desktop_vm
            && self.rootless == Some(true)
    }

    pub(crate) fn readiness_fingerprint(&self) -> String {
        use sha2::Digest as _;

        let mut available_images = self
            .available_images
            .iter()
            .map(String::as_str)
            .collect::<Vec<_>>();
        available_images.sort_unstable();
        available_images.dedup();
        let binding = (
            self.state,
            self.engine_ready,
            self.desktop_vm,
            &self.daemon_version,
            &self.architecture,
            self.rootless,
            self.cpu_limits,
            self.memory_limits,
            self.pids_limit,
            self.cgroups_supported,
            available_images,
            self.disk_available_bytes,
            &self.daemon_identity_fingerprint,
        );
        let encoded = serde_json::to_vec(&binding).expect("Docker readiness facts serialize");
        format!("{:x}", sha2::Sha256::digest(encoded))
    }

    /// Stable identity for image task IDs and receipts. The daemon ID is
    /// already reduced to a hash by the probe; readiness and image inventory
    /// are intentionally excluded so a pull does not change the task key.
    pub(crate) fn engine_identity_fingerprint(&self) -> Option<String> {
        use sha2::Digest as _;

        if !self.engine_ready {
            return None;
        }
        let endpoint = DockerEndpoint::from_process_environment(probe::host_platform())
            .ok()?
            .fingerprint();
        let daemon = self.daemon_identity_fingerprint.as_deref()?;
        if !is_sha256_hex(&endpoint) || !is_sha256_hex(daemon) {
            return None;
        }
        let identity =
            serde_json::to_vec(&("koolade-engine-identity-v1", endpoint, daemon)).ok()?;
        Some(format!("{:x}", sha2::Sha256::digest(identity)))
    }

    pub(crate) fn current_endpoint_fingerprint() -> Option<String> {
        let endpoint = DockerEndpoint::from_process_environment(host_platform()).ok()?;
        Some(endpoint_fingerprint(
            &endpoint,
            std::env::var("DOCKER_CONTEXT").ok().as_deref(),
            std::env::var("DOCKER_HOST").ok().as_deref(),
        ))
    }

    /// Stable normalized endpoint identity, independent of shell overrides
    /// that resolve to the same trusted local socket.
    pub(crate) fn current_endpoint_identity_fingerprint() -> Option<String> {
        let endpoint = DockerEndpoint::from_process_environment(host_platform()).ok()?;
        Some(endpoint.fingerprint())
    }
}

fn persist_rootless_selection(status: &mut DockerEnvironmentStatus) {
    if !cfg!(target_os = "linux") {
        return;
    }
    let selection_persisted = crate::persistence::linux_rootless_docker_selection::observe_current(
        status.endpoint_fingerprint.as_deref(),
        status.daemon_identity_fingerprint.as_deref(),
        status.rootless,
    )
    .unwrap_or(false);
    if status.local_execution_ready() && !selection_persisted {
        status.state = DockerEnvironmentState::CapabilitiesIncompatible;
        status.engine_ready = false;
        status.message = "Kool.ad/e could not confirm the current-user rootless socket identity in private local settings. Check socket ownership and application storage permissions, then recheck.".into();
    }
}

fn is_sha256_hex(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

#[cfg(test)]
#[path = "environment/tests.rs"]
mod tests;

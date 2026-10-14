//! Cached, read-only local Docker readiness for Settings and execution routing.

use std::{
    path::{Path, PathBuf},
    sync::{Mutex, OnceLock},
    time::{Duration, Instant},
};

use serde::{Deserialize, Serialize};

use super::{
    discovery::{EngineReadiness, PrerequisiteCode, PrerequisiteFailure},
    engine::{DockerEndpoint, EngineDiscovery, HostPlatform},
};

const CACHE_TTL: Duration = Duration::from_secs(15);
const PROBE_TIMEOUT: Duration = Duration::from_secs(8);
static CACHE: OnceLock<Mutex<Option<(Instant, DockerEnvironmentStatus)>>> = OnceLock::new();

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DockerEnvironmentState {
    NotInstalled,
    NotRunning,
    PermissionDenied,
    RootfulLinuxRejected,
    DesktopVmRequired,
    MissingApprovedImage,
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
    pub available_images: Vec<String>,
    pub disk_available_bytes: Option<u64>,
}

impl DockerEnvironmentStatus {
    pub fn detect() -> Self {
        let cache = CACHE.get_or_init(|| Mutex::new(None));
        if let Ok(cache) = cache.lock()
            && let Some((checked_at, status)) = &*cache
            && checked_at.elapsed() < CACHE_TTL
        {
            return status.clone();
        }
        let status = probe();
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
        let state = match failure.code {
            PrerequisiteCode::DockerMissing if desktop_host => {
                DockerEnvironmentState::DesktopVmRequired
            }
            PrerequisiteCode::DockerMissing => DockerEnvironmentState::NotInstalled,
            PrerequisiteCode::DaemonStopped => DockerEnvironmentState::NotRunning,
            PrerequisiteCode::PermissionDenied => DockerEnvironmentState::PermissionDenied,
            PrerequisiteCode::RootfulLinux => DockerEnvironmentState::RootfulLinuxRejected,
            PrerequisiteCode::UntrustedEndpoint => DockerEnvironmentState::UntrustedEndpoint,
            PrerequisiteCode::ImageUnavailable
            | PrerequisiteCode::ImageDigestMismatch
            | PrerequisiteCode::ImageArchitectureMismatch => {
                DockerEnvironmentState::MissingApprovedImage
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
            available_images: Vec::new(),
            disk_available_bytes: None,
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
            available_images: readiness.available_images,
            disk_available_bytes: readiness.disk_available_bytes,
        }
    }

    pub fn ready(&self) -> bool {
        self.engine_ready
    }
}

fn probe() -> DockerEnvironmentStatus {
    let platform = host_platform();
    let desktop_host = platform != HostPlatform::Linux;
    let endpoint = match DockerEndpoint::from_process_environment(platform) {
        Ok(endpoint) => endpoint,
        Err(failure) => return DockerEnvironmentStatus::from_failure(failure, desktop_host),
    };
    let desktop = endpoint.is_desktop();
    let engine = match EngineDiscovery::connect(endpoint) {
        Ok(engine) => engine,
        Err(failure) => return DockerEnvironmentStatus::from_failure(failure, desktop),
    };
    let Ok(runtime) = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    else {
        return DockerEnvironmentStatus::from_failure(
            PrerequisiteFailure {
                code: PrerequisiteCode::CapabilitiesIncompatible,
                message: "Could not create the Docker readiness probe runtime".into(),
                retryable: false,
            },
            desktop,
        );
    };
    let staging = match app_staging_root() {
        Ok(staging) => staging,
        Err(failure) => return DockerEnvironmentStatus::from_failure(failure, desktop),
    };
    let result = runtime.block_on(async {
        tokio::time::timeout(PROBE_TIMEOUT, engine.probe(Path::new(&staging))).await
    });
    match result {
        Ok(Ok(readiness)) => DockerEnvironmentStatus::from_readiness(readiness),
        Ok(Err(failure)) => DockerEnvironmentStatus::from_failure(failure, desktop),
        Err(_) => DockerEnvironmentStatus::from_failure(
            PrerequisiteFailure {
                code: PrerequisiteCode::DaemonStopped,
                message: "Docker Engine readiness probe timed out; start the daemon and retry"
                    .into(),
                retryable: true,
            },
            desktop,
        ),
    }
}

pub fn run_smoke_test() -> Result<(), PrerequisiteFailure> {
    let platform = host_platform();
    let endpoint = DockerEndpoint::from_process_environment(platform)?;
    let engine = EngineDiscovery::connect(endpoint)?;
    let staging = app_staging_root()?;
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|_| PrerequisiteFailure {
            code: PrerequisiteCode::CapabilitiesIncompatible,
            message: "Could not create the Docker smoke-test runtime".into(),
            retryable: false,
        })?;
    runtime.block_on(engine.test_environment(&staging))
}

fn app_staging_root() -> Result<PathBuf, PrerequisiteFailure> {
    let state_root = crate::persistence::state_root();
    let staging = state_root.join("execution-staging");
    if std::fs::symlink_metadata(&staging).is_ok_and(|metadata| metadata.file_type().is_symlink()) {
        return Err(PrerequisiteFailure {
            code: PrerequisiteCode::WorkspaceUnavailable,
            message: "App-owned execution staging cannot be a symbolic link".into(),
            retryable: false,
        });
    }
    std::fs::create_dir_all(&staging).map_err(|error| PrerequisiteFailure {
        code: PrerequisiteCode::WorkspaceUnavailable,
        message: format!("Could not prepare app-owned execution staging: {error}"),
        retryable: false,
    })?;
    let state_root = state_root.canonicalize().map_err(|_| PrerequisiteFailure {
        code: PrerequisiteCode::WorkspaceUnavailable,
        message: "Could not resolve the app-owned execution state directory".into(),
        retryable: false,
    })?;
    let staging = staging.canonicalize().map_err(|_| PrerequisiteFailure {
        code: PrerequisiteCode::WorkspaceUnavailable,
        message: "Could not resolve app-owned execution staging".into(),
        retryable: false,
    })?;
    if staging.parent() != Some(state_root.as_path()) {
        return Err(PrerequisiteFailure {
            code: PrerequisiteCode::WorkspaceUnavailable,
            message: "App-owned execution staging resolved outside the application state directory"
                .into(),
            retryable: false,
        });
    }
    Ok(staging)
}

fn host_platform() -> HostPlatform {
    if cfg!(target_os = "macos") {
        HostPlatform::MacOs
    } else if cfg!(target_os = "windows") {
        HostPlatform::Windows
    } else {
        HostPlatform::Linux
    }
}

#[cfg(test)]
#[path = "environment/tests.rs"]
mod tests;

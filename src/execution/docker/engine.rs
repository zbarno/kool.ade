use std::{
    future::Future,
    path::Path,
    pin::Pin,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

mod approval;
mod derived_image;
mod docker_api;
mod errors;
mod image_derivation;
mod snapshot;
pub(crate) use docker_api::DockerApi;
use errors::{
    daemon_storage_capacity, failure, map_api_error, map_connect_error, operation_timeout,
};

#[cfg(unix)]
use std::os::unix::fs::FileTypeExt;

use crate::execution::docker::{
    discovery::{EngineReadiness, PrerequisiteCode, PrerequisiteFailure, validate_snapshot},
    images::{ImageLock, ImageObservation, SelectedImage},
    prerequisites::{DEFAULT_MINIMUM_FREE_BYTES, inspect_workspace_capacity},
};

mod bollard_api;
pub mod endpoint;
mod smoke;
pub(crate) use approval::fingerprint as approval_fingerprint;
use bollard_api::BollardApi;
pub use endpoint::{DockerEndpoint, HostPlatform};
pub use snapshot::EngineSnapshot;

pub const PROBE_CACHE_TTL: Duration = Duration::from_secs(15);
const ENGINE_OPERATION_TIMEOUT: Duration = Duration::from_secs(10);
const IMAGE_PREPARATION_TIMEOUT: Duration = Duration::from_secs(15 * 60);

#[derive(Clone, Copy)]
pub(super) struct EngineApiTimeouts {
    pub(super) operation: Duration,
    pub(super) image_preparation: Duration,
}

impl Default for EngineApiTimeouts {
    fn default() -> Self {
        Self {
            operation: ENGINE_OPERATION_TIMEOUT,
            image_preparation: IMAGE_PREPARATION_TIMEOUT,
        }
    }
}

pub type EngineFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

struct CacheEntry {
    expires_at: Instant,
    snapshot: EngineSnapshot,
    images: Vec<String>,
}

pub struct EngineDiscovery {
    endpoint: DockerEndpoint,
    api: Arc<dyn DockerApi>,
    timeouts: EngineApiTimeouts,
    cache: Mutex<Option<CacheEntry>>,
}

impl EngineDiscovery {
    pub fn connect(endpoint: DockerEndpoint) -> Result<Self, PrerequisiteFailure> {
        if let DockerEndpoint::Unix { path, .. } = &endpoint {
            let _metadata = std::fs::symlink_metadata(path).map_err(|error| {
                let code = if error.kind() == std::io::ErrorKind::PermissionDenied { PrerequisiteCode::PermissionDenied } else { PrerequisiteCode::DockerMissing };
                failure(code, if code == PrerequisiteCode::PermissionDenied { "Permission denied reading the Docker socket" } else { "Docker Engine socket was not found; install or start a supported local Docker Engine" }, code == PrerequisiteCode::DockerMissing)
            })?;
            #[cfg(unix)]
            if !_metadata.file_type().is_socket() {
                return Err(failure(
                    PrerequisiteCode::UntrustedEndpoint,
                    "The configured Docker endpoint is not a Unix socket",
                    false,
                ));
            }
        }
        let docker = endpoint.connect()?;
        let api = Arc::new(BollardApi::new(
            docker,
            endpoint.clone(),
            DEFAULT_MINIMUM_FREE_BYTES,
        ));
        Ok(Self::with_api(endpoint, api))
    }

    #[cfg(unix)]
    pub(crate) fn connect_pinned(endpoint: DockerEndpoint) -> Result<Self, PrerequisiteFailure> {
        let docker = endpoint.connect_pinned()?;
        let api = Arc::new(BollardApi::new(
            docker,
            endpoint.clone(),
            DEFAULT_MINIMUM_FREE_BYTES,
        ));
        Ok(Self::with_api(endpoint, api))
    }

    pub(crate) fn with_api(endpoint: DockerEndpoint, api: Arc<dyn DockerApi>) -> Self {
        Self::with_api_timeouts(endpoint, api, EngineApiTimeouts::default())
    }

    pub(super) fn with_api_timeouts(
        endpoint: DockerEndpoint,
        api: Arc<dyn DockerApi>,
        timeouts: EngineApiTimeouts,
    ) -> Self {
        Self {
            endpoint,
            api,
            timeouts,
            cache: Mutex::new(None),
        }
    }

    /// Call after the selected Docker context changes or a daemon restart is observed.
    pub fn invalidate(&self) {
        if let Ok(mut cache) = self.cache.lock() {
            *cache = None;
        }
    }

    pub async fn probe(&self, workspace: &Path) -> Result<EngineReadiness, PrerequisiteFailure> {
        let image_lock = ImageLock::bundled()?;
        let cached = self.cache.lock().ok().and_then(|cache| {
            cache
                .as_ref()
                .filter(|entry| entry.expires_at > Instant::now())
                .map(|entry| (entry.snapshot.clone(), entry.images.clone()))
        });
        let (snapshot, observed_images) = if let Some(cached) = cached {
            cached
        } else {
            let snapshot = tokio::time::timeout(self.timeouts.operation, self.api.inspect())
                .await
                .map_err(|_| operation_timeout("Docker Engine health inspection"))??;
            let images = tokio::time::timeout(self.timeouts.operation, self.api.image_digests())
                .await
                .map_err(|_| operation_timeout("Docker image inventory"))??;
            if let Ok(mut cache) = self.cache.lock() {
                *cache = Some(CacheEntry {
                    expires_at: Instant::now() + PROBE_CACHE_TTL,
                    snapshot: snapshot.clone(),
                    images: images.clone(),
                });
            }
            (snapshot, images)
        };
        let images = crate::execution::docker::images::approved_available_images(
            &image_lock,
            &observed_images,
        );
        let mut result = validate_snapshot(&snapshot).err();
        let workspace_capacity = inspect_workspace_capacity(workspace);
        if result.is_none() {
            result = match (
                workspace_capacity.accessible,
                workspace_capacity.available_bytes,
            ) {
                (false, _) => Some(failure(
                    PrerequisiteCode::WorkspaceUnavailable,
                    "The application-owned task staging directory is unavailable",
                    false,
                )),
                (true, None) => Some(failure(
                    PrerequisiteCode::InsufficientDisk,
                    "Free space for task staging could not be verified",
                    false,
                )),
                (true, Some(bytes)) if bytes < snapshot.minimum_disk_bytes => Some(failure(
                    PrerequisiteCode::InsufficientDisk,
                    "Task staging has less free space than required",
                    true,
                )),
                _ => None,
            };
        }
        let disk_available_bytes = match (
            snapshot.disk_available_bytes,
            workspace_capacity.available_bytes,
        ) {
            (Some(engine), Some(staging)) => Some(engine.min(staging)),
            _ => None,
        };
        Ok(EngineReadiness {
            ready: result.is_none(),
            daemon_identity_fingerprint: (!snapshot.daemon_id.is_empty()).then(|| {
                use sha2::Digest as _;
                format!("{:x}", sha2::Sha256::digest(snapshot.daemon_id.as_bytes()))
            }),
            docker_desktop: self.endpoint.is_desktop() || snapshot.docker_desktop,
            daemon_version: Some(snapshot.daemon_version),
            api_version: Some(snapshot.api_version),
            architecture: Some(snapshot.architecture),
            rootless: Some(snapshot.rootless),
            linux_containers: Some(snapshot.os_type.eq_ignore_ascii_case("linux")),
            seccomp_enabled: Some(snapshot.seccomp_enabled),
            memory_limit: Some(snapshot.memory_limit),
            cpu_quota: Some(snapshot.cpu_quota),
            pids_limit: Some(snapshot.pids_limit),
            cgroups_supported: match (
                snapshot.cgroup_version.as_deref(),
                snapshot.cgroup_driver.as_deref(),
            ) {
                (Some("1" | "2"), Some("systemd" | "cgroupfs")) => Some(true),
                (Some(_), Some(_)) => Some(false),
                _ => None,
            },
            workspace_staging_available: workspace_capacity.accessible,
            disk_available_bytes,
            available_images: images,
            failure: result,
        })
    }

    /// Return one bounded, validated daemon identity snapshot for consent
    /// binding. Callers must re-probe before using the resulting identity.
    pub async fn inspect_snapshot(&self) -> Result<EngineSnapshot, PrerequisiteFailure> {
        let snapshot = tokio::time::timeout(self.timeouts.operation, self.api.inspect())
            .await
            .map_err(|_| operation_timeout("Docker Engine identity inspection"))??;
        validate_snapshot(&snapshot)?;
        Ok(snapshot)
    }

    /// Pulls only an image selected from the bundled digest lock. Health probes never pull images.
    pub async fn prepare_image(&self, image: &SelectedImage) -> Result<(), PrerequisiteFailure> {
        self.prepare_image_with_fingerprint(image, None).await
    }

    async fn prepare_image_with_fingerprint(
        &self,
        image: &SelectedImage,
        expected_engine_fingerprint: Option<&str>,
    ) -> Result<(), PrerequisiteFailure> {
        let lock = ImageLock::bundled()?;
        let architecture = image.platform.strip_prefix("linux/").unwrap_or_default();
        let expected = lock.select(image.profile, architecture)?;
        if expected != *image {
            return Err(failure(
                PrerequisiteCode::ImageDigestMismatch,
                "Requested image does not match the bundled approved digest lock",
                false,
            ));
        }
        let observed = tokio::time::timeout(
            self.timeouts.image_preparation,
            self.api.ensure_image(image, expected_engine_fingerprint),
        )
        .await
        .map_err(|_| operation_timeout("trusted executor image preparation"))??;
        crate::execution::docker::images::verify_image_observation(
            image,
            &observed.repo_digests,
            &observed.os,
            &observed.architecture,
        )?;
        Ok(())
    }

    pub fn endpoint(&self) -> &DockerEndpoint {
        &self.endpoint
    }
}

#[cfg(test)]
#[path = "engine/tests/smoke.rs"]
mod smoke_tests;

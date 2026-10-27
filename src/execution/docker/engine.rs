use std::{
    future::Future,
    pin::Pin,
    sync::{Arc, Mutex},
    time::Duration,
};

mod approval;
mod derived_image;
mod docker_api;
mod errors;
mod image_derivation;
mod image_inspection;
mod image_preparation;
mod image_pull;
mod probe;
mod snapshot;
pub(crate) use docker_api::DockerApi;
#[cfg(unix)]
use errors::map_connect_error;
use errors::{daemon_storage_capacity, failure, map_api_error, operation_timeout};

#[cfg(unix)]
use std::os::unix::fs::FileTypeExt;

use crate::execution::docker::{
    discovery::{PrerequisiteCode, PrerequisiteFailure, validate_snapshot},
    images::{ImageLock, ImageObservation, RuntimeProfile, SelectedImage},
    prerequisites::DEFAULT_MINIMUM_FREE_BYTES,
};

mod bollard_api;
pub mod endpoint;
mod smoke;
pub(crate) use approval::fingerprint as approval_fingerprint;
use bollard_api::BollardApi;
pub use endpoint::{DockerEndpoint, HostPlatform};
pub(crate) use image_pull::{ImagePullControl, ImagePullProgress};
use probe::{CacheEntry, DesktopCapacitySample};
pub use snapshot::EngineSnapshot;

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

pub struct EngineDiscovery {
    endpoint: DockerEndpoint,
    desktop_transport_candidate: bool,
    api: Arc<dyn DockerApi>,
    timeouts: EngineApiTimeouts,
    cache: Mutex<Option<CacheEntry>>,
    desktop_capacity_cache: Mutex<Option<DesktopCapacitySample>>,
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
        let mut discovery = Self::with_api(endpoint.clone(), api);
        discovery.desktop_transport_candidate =
            endpoint.is_desktop() && cfg!(any(target_os = "macos", target_os = "windows"));
        Ok(discovery)
    }

    #[cfg(unix)]
    pub(crate) fn connect_pinned(endpoint: DockerEndpoint) -> Result<Self, PrerequisiteFailure> {
        let docker = endpoint.connect_pinned()?;
        let api = Arc::new(BollardApi::new(
            docker,
            endpoint.clone(),
            DEFAULT_MINIMUM_FREE_BYTES,
        ));
        let mut discovery = Self::with_api(endpoint.clone(), api);
        discovery.desktop_transport_candidate = endpoint.is_desktop() && cfg!(target_os = "macos");
        Ok(discovery)
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
            desktop_transport_candidate: false,
            api,
            timeouts,
            cache: Mutex::new(None),
            desktop_capacity_cache: Mutex::new(None),
        }
    }

    #[cfg(test)]
    pub(crate) fn with_test_verified_desktop_transport(
        endpoint: DockerEndpoint,
        api: Arc<dyn DockerApi>,
    ) -> Self {
        let mut discovery = Self::with_api(endpoint, api);
        discovery.desktop_transport_candidate = true;
        discovery
    }

    /// Call after the selected Docker context changes or a daemon restart is observed.
    pub fn invalidate(&self) {
        if let Ok(mut cache) = self.cache.lock() {
            *cache = None;
        }
        if let Ok(mut cache) = self.desktop_capacity_cache.lock() {
            *cache = None;
        }
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

    /// Checks a locked image that is already local. Pulls require an approved action.
    pub async fn prepare_image(&self, image: &SelectedImage) -> Result<(), PrerequisiteFailure> {
        self.verify_image_present(image).await
    }

    pub fn endpoint(&self) -> &DockerEndpoint {
        &self.endpoint
    }
}

#[cfg(test)]
#[path = "engine/tests/smoke.rs"]
mod smoke_tests;

use std::{path::Path, time::Instant};

use super::{EngineDiscovery, EngineSnapshot, failure, operation_timeout};
use crate::execution::docker::{
    discovery::{
        EngineReadiness, PrerequisiteCode, PrerequisiteFailure, validate_snapshot,
        validate_snapshot_engine,
    },
    images::{ImageLock, RuntimeProfile, approved_available_images},
    prerequisites::inspect_workspace_capacity,
};

const PROBE_CACHE_TTL: std::time::Duration = std::time::Duration::from_secs(15);

#[derive(Clone)]
pub(super) struct CacheEntry {
    expires_at: Instant,
    snapshot: EngineSnapshot,
    images: Vec<String>,
}

#[derive(Clone)]
pub(super) struct DesktopCapacitySample {
    expires_at: Instant,
    endpoint_fingerprint: String,
    daemon_identity_fingerprint: String,
    available_bytes: u64,
}

impl EngineDiscovery {
    pub async fn probe(&self, workspace: &Path) -> Result<EngineReadiness, PrerequisiteFailure> {
        let image_lock = ImageLock::bundled()?;
        let desktop_candidate = self.desktop_transport_candidate && self.endpoint.is_desktop();
        let cached_entry = self.cache.lock().ok().and_then(|cache| {
            cache
                .as_ref()
                .filter(|entry| entry.expires_at > Instant::now())
                .cloned()
        });
        let cached_desktop_sample = if desktop_candidate {
            self.desktop_capacity_cache.lock().ok().and_then(|cache| {
                cache
                    .as_ref()
                    .filter(|sample| sample.expires_at > Instant::now())
                    .cloned()
            })
        } else {
            None
        };
        // Desktop snapshots and image inventory are rechecked on every probe.
        // A short-lived guest sample is reusable only after matching the fresh
        // endpoint and daemon identity below.
        let cached = if desktop_candidate {
            None
        } else {
            cached_entry
                .as_ref()
                .map(|entry| (entry.snapshot.clone(), entry.images.clone()))
        };
        let (mut snapshot, observed_images) = if let Some(cached) = cached {
            cached
        } else {
            let snapshot = tokio::time::timeout(self.timeouts.operation, self.api.inspect())
                .await
                .map_err(|_| operation_timeout("Docker Engine health inspection"))??;
            let images = tokio::time::timeout(self.timeouts.operation, self.api.image_digests())
                .await
                .map_err(|_| operation_timeout("Docker image inventory"))??;
            if !desktop_candidate && let Ok(mut cache) = self.cache.lock() {
                *cache = Some(CacheEntry {
                    expires_at: Instant::now() + PROBE_CACHE_TTL,
                    snapshot: snapshot.clone(),
                    images: images.clone(),
                });
            }
            (snapshot, images)
        };
        let mut desktop_capacity_measured = false;
        let images = approved_available_images(&image_lock, &observed_images);
        if self.endpoint.is_desktop() {
            // Desktop Engine metadata may describe host-side or driver-specific
            // storage. Only the isolated guest probe below can establish VM bytes.
            snapshot.docker_storage_accessible = false;
            snapshot.disk_available_bytes = None;
        }
        let desktop_transport_verified = self.desktop_transport_candidate
            && self.endpoint.is_desktop()
            && snapshot.docker_desktop
            && validate_snapshot_engine(&snapshot).is_ok();
        let desktop_capacity_failure = if desktop_transport_verified {
            match image_lock.select(RuntimeProfile::General, &snapshot.architecture) {
                Ok(general_image) if images.contains(&general_image.reference) => {
                    if !desktop_capacity_measured
                        && !snapshot.daemon_id.is_empty()
                        && let Some(cached_sample) =
                            cached_desktop_sample.as_ref().filter(|sample| {
                                sample.endpoint_fingerprint == self.endpoint.fingerprint()
                                    && sample.daemon_identity_fingerprint
                                        == daemon_identity_fingerprint(&snapshot.daemon_id)
                            })
                    {
                        snapshot.docker_storage_accessible = true;
                        snapshot.disk_available_bytes = Some(cached_sample.available_bytes);
                        desktop_capacity_measured = true;
                    }
                    if desktop_capacity_measured {
                        None
                    } else {
                        match self
                            .measure_desktop_storage_capacity(&mut snapshot, &images)
                            .await
                        {
                            Ok(()) => {
                                desktop_capacity_measured = true;
                                if let Some(available_bytes) = snapshot.disk_available_bytes
                                    && !snapshot.daemon_id.is_empty()
                                    && let Ok(mut cache) = self.desktop_capacity_cache.lock()
                                {
                                    *cache = Some(DesktopCapacitySample {
                                        expires_at: Instant::now() + PROBE_CACHE_TTL,
                                        endpoint_fingerprint: self.endpoint.fingerprint(),
                                        daemon_identity_fingerprint: daemon_identity_fingerprint(
                                            &snapshot.daemon_id,
                                        ),
                                        available_bytes,
                                    });
                                }
                                None
                            }
                            Err(failure) => Some(failure),
                        }
                    }
                }
                Ok(_) => Some(failure(
                    PrerequisiteCode::InsufficientDisk,
                    "Docker Desktop VM capacity cannot be measured until the exact locked General image is installed locally",
                    true,
                )),
                Err(failure) => Some(failure),
            }
        } else {
            None
        };
        let mut result = desktop_capacity_failure.or_else(|| validate_snapshot(&snapshot).err());
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
            desktop_transport_verified,
            desktop_vm_capacity_verified: self.endpoint.is_desktop()
                && desktop_transport_verified
                && desktop_capacity_measured,
            daemon_identity_fingerprint: (!snapshot.daemon_id.is_empty())
                .then(|| daemon_identity_fingerprint(&snapshot.daemon_id)),
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
            workspace_staging_available: workspace_capacity.accessible
                && workspace_capacity
                    .available_bytes
                    .is_some_and(|available| available >= snapshot.minimum_disk_bytes),
            disk_available_bytes,
            available_images: images,
            failure: result,
        })
    }
}

fn daemon_identity_fingerprint(daemon_id: &str) -> String {
    use sha2::Digest as _;
    format!("{:x}", sha2::Sha256::digest(daemon_id.as_bytes()))
}

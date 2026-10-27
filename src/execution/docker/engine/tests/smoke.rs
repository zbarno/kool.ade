use super::*;
use std::path::Path;
use std::sync::{
    Mutex,
    atomic::{AtomicUsize, Ordering},
};

use crate::execution::docker::images::{ImageObservation, RuntimeProfile, SelectedImage};

#[path = "smoke/approved.rs"]
mod approved;
#[path = "smoke/desktop_bootstrap.rs"]
mod desktop_bootstrap;
#[path = "smoke/environment.rs"]
mod environment;

struct SmokeApi {
    smoke_image: Mutex<Option<String>>,
    smoke_name: Mutex<Option<String>>,
    derived_observation: Mutex<Option<ImageObservation>>,
    daemon_id: Mutex<String>,
    pull_count: AtomicUsize,
    swap_after_pull: Mutex<Option<String>>,
    swap_after_smoke: Mutex<Option<String>>,
    available_images: Mutex<Vec<String>>,
    tamper_after_smoke: bool,
    cancel_after_smoke: Mutex<Option<ImagePullControl>>,
}

fn pull_control() -> ImagePullControl {
    let (progress, _receiver) = std::sync::mpsc::sync_channel(8);
    ImagePullControl::new(progress)
}

impl DockerApi for SmokeApi {
    fn inspect<'a>(&'a self) -> EngineFuture<'a, Result<EngineSnapshot, PrerequisiteFailure>> {
        let daemon_id = self.daemon_id.lock().unwrap().clone();
        Box::pin(async move {
            Ok(EngineSnapshot {
                daemon_id,
                daemon_version: "28.0.0".into(),
                api_version: "1.53".into(),
                minimum_api_version: "1.41".into(),
                os_type: "linux".into(),
                architecture: "amd64".into(),
                security_options: vec!["name=rootless".into(), "name=seccomp".into()],
                rootless: true,
                native_linux: true,
                docker_desktop: false,
                memory_limit: true,
                cpu_quota: true,
                pids_limit: true,
                cgroup_version: Some("2".into()),
                cgroup_driver: Some("systemd".into()),
                seccomp_enabled: true,
                docker_storage_accessible: true,
                disk_available_bytes: Some(8 * 1024 * 1024 * 1024),
                minimum_disk_bytes: 1024,
            })
        })
    }

    fn image_digests<'a>(&'a self) -> EngineFuture<'a, Result<Vec<String>, PrerequisiteFailure>> {
        Box::pin(async move { Ok(self.available_images.lock().unwrap().clone()) })
    }

    fn inspect_image<'a>(
        &'a self,
        _image: &'a SelectedImage,
    ) -> EngineFuture<'a, Result<ImageObservation, PrerequisiteFailure>> {
        Box::pin(async move {
            self.derived_observation
                .lock()
                .unwrap()
                .clone()
                .ok_or_else(|| {
                    failure(
                        PrerequisiteCode::ImageUnavailable,
                        "missing fake image",
                        false,
                    )
                })
        })
    }

    fn ensure_image<'a>(
        &'a self,
        image: &'a SelectedImage,
        _expected_engine_fingerprint: Option<&'a str>,
    ) -> EngineFuture<'a, Result<ImageObservation, PrerequisiteFailure>> {
        Box::pin(async move {
            self.pull_count.fetch_add(1, Ordering::Relaxed);
            let mut available_images = self.available_images.lock().unwrap();
            let docker_hub_alias = image.reference.replace("docker.io/library/", "");
            if !available_images.contains(&docker_hub_alias) {
                available_images.push(image.reference.clone());
            }
            drop(available_images);
            if let Some(daemon_id) = self.swap_after_pull.lock().unwrap().take() {
                *self.daemon_id.lock().unwrap() = daemon_id;
            }
            let observation = ImageObservation {
                image_id: image.digest.clone(),
                repo_digests: vec![image.reference.clone()],
                os: "linux".into(),
                architecture: image.platform.strip_prefix("linux/").unwrap().into(),
                configured_user: "10001:10001".into(),
                labels: Default::default(),
            };
            *self.derived_observation.lock().unwrap() = Some(observation.clone());
            Ok(observation)
        })
    }

    fn ensure_image_with_control<'a>(
        &'a self,
        image: &'a SelectedImage,
        expected_engine_fingerprint: &'a str,
        _verified_capacity_bytes: Option<u64>,
        control: &'a ImagePullControl,
    ) -> EngineFuture<'a, Result<ImageObservation, PrerequisiteFailure>> {
        Box::pin(async move {
            if control.is_cancelled() {
                return Err(failure(
                    PrerequisiteCode::ImageUnavailable,
                    "approved image pull was cancelled",
                    true,
                ));
            }
            self.ensure_image(image, Some(expected_engine_fingerprint))
                .await
        })
    }

    fn inspect_derived_image<'a>(
        &'a self,
        _image: &'a SelectedImage,
    ) -> EngineFuture<'a, Result<ImageObservation, PrerequisiteFailure>> {
        Box::pin(async move {
            self.derived_observation
                .lock()
                .unwrap()
                .clone()
                .ok_or_else(|| {
                    failure(
                        PrerequisiteCode::ImageUnavailable,
                        "missing fake image",
                        false,
                    )
                })
        })
    }

    fn smoke_test<'a>(
        &'a self,
        image: &'a SelectedImage,
        container_name: &'a str,
        _expected_engine_fingerprint: Option<&'a str>,
    ) -> EngineFuture<'a, Result<(), PrerequisiteFailure>> {
        Box::pin(async move {
            *self.smoke_image.lock().unwrap() = Some(image.reference.clone());
            *self.smoke_name.lock().unwrap() = Some(container_name.to_owned());
            if self.tamper_after_smoke
                && let Some(observation) = self.derived_observation.lock().unwrap().as_mut()
            {
                let repository = image.reference.rsplit_once('@').unwrap().0;
                observation.repo_digests = vec![format!("{repository}@sha256:{}", "0".repeat(64))];
            }
            if let Some(daemon_id) = self.swap_after_smoke.lock().unwrap().take() {
                *self.daemon_id.lock().unwrap() = daemon_id;
            }
            if let Some(control) = self.cancel_after_smoke.lock().unwrap().take() {
                control.cancel();
            }
            Ok(())
        })
    }
}

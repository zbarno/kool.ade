use super::*;
use std::sync::Mutex;

use crate::execution::docker::{images::ImageObservation, images::SelectedImage};

struct SmokeApi {
    smoke_image: Mutex<Option<String>>,
    smoke_name: Mutex<Option<String>>,
    derived_observation: Mutex<Option<ImageObservation>>,
}

impl DockerApi for SmokeApi {
    fn inspect<'a>(&'a self) -> EngineFuture<'a, Result<EngineSnapshot, PrerequisiteFailure>> {
        Box::pin(async {
            Ok(EngineSnapshot {
                daemon_id: "fake-daemon".into(),
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
                seccomp_enabled: true,
                docker_storage_accessible: true,
                disk_available_bytes: Some(8 * 1024 * 1024 * 1024),
                minimum_disk_bytes: 1024,
            })
        })
    }

    fn image_digests<'a>(&'a self) -> EngineFuture<'a, Result<Vec<String>, PrerequisiteFailure>> {
        Box::pin(async { Ok(Vec::new()) })
    }

    fn ensure_image<'a>(
        &'a self,
        image: &'a SelectedImage,
    ) -> EngineFuture<'a, Result<ImageObservation, PrerequisiteFailure>> {
        Box::pin(async move {
            Ok(ImageObservation {
                image_id: image.digest.clone(),
                repo_digests: vec![image.reference.clone()],
                os: "linux".into(),
                architecture: image.platform.strip_prefix("linux/").unwrap().into(),
                configured_user: "10001:10001".into(),
                labels: Default::default(),
            })
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
    ) -> EngineFuture<'a, Result<(), PrerequisiteFailure>> {
        Box::pin(async move {
            *self.smoke_image.lock().unwrap() = Some(image.reference.clone());
            *self.smoke_name.lock().unwrap() = Some(container_name.to_owned());
            Ok(())
        })
    }
}

#[test]
fn environment_test_uses_general_locked_image_and_scoped_smoke_name() {
    let api = std::sync::Arc::new(SmokeApi {
        smoke_image: Mutex::new(None),
        smoke_name: Mutex::new(None),
        derived_observation: Mutex::new(None),
    });
    let engine = EngineDiscovery::with_api(
        DockerEndpoint::Unix {
            path: "/tmp/fake-docker.sock".into(),
            desktop: false,
        },
        api.clone(),
    );
    let staging = tempfile::tempdir().unwrap();
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(engine.test_environment(staging.path()))
        .unwrap();

    assert!(
        api.smoke_image
            .lock()
            .unwrap()
            .as_deref()
            .is_some_and(|image| image.contains("@sha256:"))
    );
    let name = api.smoke_name.lock().unwrap().clone().unwrap();
    assert!(name.starts_with("koolade-smoke-"));
    assert_eq!(name.len(), "koolade-smoke-".len() + 32);
}

#[test]
fn registered_derived_image_is_inspected_without_pull_and_mismatches_fail_closed() {
    let image = SelectedImage {
        profile: crate::execution::docker::images::RuntimeProfile::General,
        platform: "linux/amd64".into(),
        reference: format!("sha256:{}", "1".repeat(64)),
        digest: format!("sha256:{}", "1".repeat(64)),
        runtime_user: "10001:10001".into(),
        tool_versions: Default::default(),
    };
    let content_identity = format!("sha256:{}", "2".repeat(64));
    let base_digest = format!("sha256:{}", "3".repeat(64));
    let api = std::sync::Arc::new(SmokeApi {
        smoke_image: Mutex::new(None),
        smoke_name: Mutex::new(None),
        derived_observation: Mutex::new(Some(ImageObservation {
            image_id: image.digest.clone(),
            repo_digests: Vec::new(),
            os: "linux".into(),
            architecture: "amd64".into(),
            configured_user: "10001:10001".into(),
            labels: std::collections::BTreeMap::from([
                ("io.koolade.content".into(), content_identity.clone()),
                ("io.koolade.base-digest".into(), base_digest.clone()),
            ]),
        })),
    });
    let engine = EngineDiscovery::with_api(
        DockerEndpoint::Unix {
            path: "/tmp/fake.sock".into(),
            desktop: false,
        },
        api.clone(),
    );
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    runtime
        .block_on(engine.prepare_derived_image(&image, &content_identity, &base_digest))
        .unwrap();

    api.derived_observation
        .lock()
        .unwrap()
        .as_mut()
        .unwrap()
        .image_id = format!("sha256:{}", "4".repeat(64));
    assert_eq!(
        runtime
            .block_on(engine.prepare_derived_image(&image, &content_identity, &base_digest))
            .unwrap_err()
            .code,
        PrerequisiteCode::ImageDigestMismatch
    );
}

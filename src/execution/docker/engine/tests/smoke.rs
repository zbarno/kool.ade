use super::*;
use std::sync::Mutex;

use crate::execution::docker::{images::ImageObservation, images::SelectedImage};

struct SmokeApi {
    smoke_image: Mutex<Option<String>>,
    smoke_name: Mutex<Option<String>>,
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
                repo_digests: vec![image.reference.clone()],
                os: "linux".into(),
                architecture: image.platform.strip_prefix("linux/").unwrap().into(),
                configured_user: "10001:10001".into(),
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

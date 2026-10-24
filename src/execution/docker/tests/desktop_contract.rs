//! Production boundary: host selection produces a Docker Desktop endpoint,
//! which `EngineDiscovery` combines with API observations for Settings. These
//! tests prove the endpoint's Desktop identity reaches readiness and that the
//! approved image inventory and fail-closed VM checks are preserved; testing
//! `validate_snapshot` alone would not cover that endpoint-to-readiness path.
//! The Docker API is fake, with a temporary local staging directory. No Docker,
//! SSH, or network is required; setup cost is one small temporary directory.
//! This does not prove real Desktop transport or host-path staging behavior.

use super::*;
use crate::execution::docker::engine::HostPlatform;

struct DesktopApi {
    snapshot: EngineSnapshot,
    images: Vec<String>,
}

impl DockerApi for DesktopApi {
    fn inspect<'a>(&'a self) -> EngineFuture<'a, Result<EngineSnapshot, PrerequisiteFailure>> {
        let snapshot = self.snapshot.clone();
        Box::pin(async move { Ok(snapshot) })
    }

    fn image_digests<'a>(&'a self) -> EngineFuture<'a, Result<Vec<String>, PrerequisiteFailure>> {
        let images = self.images.clone();
        Box::pin(async move { Ok(images) })
    }

    fn ensure_image<'a>(
        &'a self,
        _image: &'a SelectedImage,
        _expected_engine_fingerprint: Option<&'a str>,
    ) -> EngineFuture<'a, Result<ImageObservation, PrerequisiteFailure>> {
        Box::pin(async {
            Err(PrerequisiteFailure {
                code: PrerequisiteCode::ImageUnavailable,
                message: "fake Desktop fixture does not pull images".into(),
                retryable: false,
            })
        })
    }
}

fn desktop_snapshot() -> EngineSnapshot {
    let mut snapshot = snapshot();
    snapshot.rootless = false;
    snapshot.native_linux = false;
    snapshot.docker_desktop = true;
    snapshot.minimum_disk_bytes = 1;
    snapshot
}

fn desktop_api(snapshot: EngineSnapshot) -> Arc<DesktopApi> {
    let image = crate::execution::docker::images::ImageLock::bundled()
        .unwrap()
        .select(
            crate::execution::docker::images::RuntimeProfile::General,
            "amd64",
        )
        .unwrap();
    Arc::new(DesktopApi {
        snapshot,
        images: vec![image.reference],
    })
}

#[test]
fn desktop_endpoints_report_ready_linux_vm_and_approved_image() {
    let expected_image = crate::execution::docker::images::ImageLock::bundled()
        .unwrap()
        .select(
            crate::execution::docker::images::RuntimeProfile::General,
            "amd64",
        )
        .unwrap()
        .reference;
    let home = std::path::Path::new("/Users/example");
    let mac =
        DockerEndpoint::from_environment(None, None, None, Some(home), None, HostPlatform::MacOs)
            .unwrap();
    let windows =
        DockerEndpoint::from_environment(None, None, None, None, None, HostPlatform::Windows)
            .unwrap();

    for endpoint in [mac, windows] {
        let discovery = EngineDiscovery::with_api(endpoint, desktop_api(desktop_snapshot()));
        let workspace = tempfile::tempdir().unwrap();
        let readiness = block_on(discovery.probe(workspace.path())).unwrap();

        assert!(
            readiness.ready,
            "Desktop Linux VM should pass: {readiness:?}"
        );
        assert!(readiness.docker_desktop);
        assert_eq!(readiness.linux_containers, Some(true));
        assert_eq!(readiness.available_images, vec![expected_image.clone()]);
        assert!(readiness.failure.is_none());
    }
}

#[test]
fn desktop_vm_storage_failure_is_reported_without_host_disk_fallback() {
    let endpoint = DockerEndpoint::NamedPipe {
        path: r"\\.\pipe\docker_engine".into(),
        desktop: true,
    };
    let mut vm = desktop_snapshot();
    vm.docker_storage_accessible = false;
    vm.disk_available_bytes = None;
    let discovery = EngineDiscovery::with_api(endpoint, desktop_api(vm));
    let workspace = tempfile::tempdir().unwrap();

    let readiness = block_on(discovery.probe(workspace.path())).unwrap();

    assert!(!readiness.ready);
    assert!(readiness.docker_desktop);
    let failure = readiness.failure.unwrap();
    assert_eq!(failure.code, PrerequisiteCode::InsufficientDisk);
    assert!(
        failure
            .message
            .contains("Docker Desktop's VM storage capacity")
    );
}

#[test]
fn desktop_vm_rejects_windows_containers() {
    let endpoint = DockerEndpoint::NamedPipe {
        path: r"\\.\pipe\docker_engine".into(),
        desktop: true,
    };
    let mut vm = desktop_snapshot();
    vm.os_type = "windows".into();
    let discovery = EngineDiscovery::with_api(endpoint, desktop_api(vm));
    let workspace = tempfile::tempdir().unwrap();

    let readiness = block_on(discovery.probe(workspace.path())).unwrap();

    assert!(!readiness.ready);
    assert_eq!(
        readiness.failure.unwrap().code,
        PrerequisiteCode::WindowsContainers
    );
}

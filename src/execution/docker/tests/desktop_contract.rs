//! Production boundary: host selection produces a Docker Desktop endpoint,
//! which `EngineDiscovery` combines with API observations for Settings. These
//! tests prove the EngineDiscovery boundary uses a fresh Desktop snapshot,
//! probes capacity only with the exact installed General digest, and scopes the
//! short-lived sample to endpoint and daemon identity. They do not certify real
//! Desktop transport or unlock app execution. Testing `validate_snapshot` alone
//! cannot cover that endpoint-to-readiness path.
//! The Docker API is fake, with a temporary local staging directory. No Docker,
//! SSH, or network is required; setup cost is one small temporary directory.
//! This does not prove real Desktop transport or host-path staging behavior.

use super::*;
use crate::execution::docker::engine::HostPlatform;
use std::sync::atomic::{AtomicUsize, Ordering};

#[path = "desktop_contract/cache.rs"]
mod cache;

struct DesktopApi {
    snapshot: std::sync::Mutex<EngineSnapshot>,
    images: Vec<String>,
    capacity_calls: Arc<AtomicUsize>,
    capacity_bytes: Option<u64>,
}

impl DockerApi for DesktopApi {
    fn inspect<'a>(&'a self) -> EngineFuture<'a, Result<EngineSnapshot, PrerequisiteFailure>> {
        let snapshot = self.snapshot.lock().unwrap().clone();
        Box::pin(async move { Ok(snapshot) })
    }

    fn image_digests<'a>(&'a self) -> EngineFuture<'a, Result<Vec<String>, PrerequisiteFailure>> {
        let images = self.images.clone();
        Box::pin(async move { Ok(images) })
    }

    fn desktop_storage_capacity<'a>(
        &'a self,
        _image: &'a SelectedImage,
        _expected_engine_fingerprint: &'a str,
    ) -> EngineFuture<'a, Result<u64, PrerequisiteFailure>> {
        self.capacity_calls.fetch_add(1, Ordering::SeqCst);
        let capacity_bytes = self.capacity_bytes;
        Box::pin(async move {
            capacity_bytes.ok_or_else(|| PrerequisiteFailure {
                code: PrerequisiteCode::InsufficientDisk,
                message: "synthetic guest storage capacity was not available".into(),
                retryable: true,
            })
        })
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
    desktop_api_with_images(snapshot, vec![image.reference])
}

fn desktop_api_with_images(snapshot: EngineSnapshot, images: Vec<String>) -> Arc<DesktopApi> {
    desktop_api_fixture(snapshot, images, None)
}

fn desktop_api_with_capacity(snapshot: EngineSnapshot, available_bytes: u64) -> Arc<DesktopApi> {
    let image = crate::execution::docker::images::ImageLock::bundled()
        .unwrap()
        .select(
            crate::execution::docker::images::RuntimeProfile::General,
            "amd64",
        )
        .unwrap();
    desktop_api_fixture(snapshot, vec![image.reference], Some(available_bytes))
}

fn desktop_api_fixture(
    snapshot: EngineSnapshot,
    images: Vec<String>,
    capacity_bytes: Option<u64>,
) -> Arc<DesktopApi> {
    Arc::new(DesktopApi {
        snapshot: std::sync::Mutex::new(snapshot),
        images,
        capacity_calls: Arc::new(AtomicUsize::new(0)),
        capacity_bytes,
    })
}

#[test]
fn synthetic_desktop_endpoint_does_not_bypass_transport_or_storage_checks() {
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
        let api = desktop_api(desktop_snapshot());
        let discovery = EngineDiscovery::with_api(endpoint, api.clone());
        let workspace = tempfile::tempdir().unwrap();
        let readiness = block_on(discovery.probe(workspace.path())).unwrap();

        assert!(!readiness.ready);
        assert!(readiness.docker_desktop);
        assert!(
            !readiness.desktop_transport_verified,
            "a synthetic Engine API result alone must not attest transport provenance"
        );
        assert_eq!(readiness.linux_containers, Some(true));
        assert_eq!(readiness.available_images, vec![expected_image.clone()]);
        assert_eq!(
            readiness.failure.unwrap().code,
            PrerequisiteCode::InsufficientDisk
        );
        assert_eq!(api.capacity_calls.load(Ordering::SeqCst), 0);
    }
}

#[test]
fn verified_transport_and_linux_engine_do_not_bypass_vm_storage_gate() {
    let endpoint = DockerEndpoint::NamedPipe {
        path: r"\\.\pipe\docker_engine".into(),
        desktop: true,
    };
    let mut vm = desktop_snapshot();
    vm.docker_storage_accessible = false;
    vm.disk_available_bytes = None;
    let api = desktop_api(vm);
    let discovery = EngineDiscovery::with_test_verified_desktop_transport(endpoint, api.clone());
    let workspace = tempfile::tempdir().unwrap();

    let readiness = block_on(discovery.probe(workspace.path())).unwrap();

    assert!(readiness.desktop_transport_verified);
    assert!(!readiness.ready);
    assert_eq!(
        readiness.failure.unwrap().code,
        PrerequisiteCode::InsufficientDisk
    );
    assert!(!readiness.desktop_vm_capacity_verified);
    assert_eq!(api.capacity_calls.load(Ordering::SeqCst), 1);
}

#[test]
fn desktop_capacity_probe_requires_the_exact_locked_general_image() {
    let endpoint = DockerEndpoint::NamedPipe {
        path: r"\\.\pipe\docker_engine".into(),
        desktop: true,
    };
    let api = desktop_api_with_images(desktop_snapshot(), vec!["unlocked-image".into()]);
    let discovery = EngineDiscovery::with_test_verified_desktop_transport(endpoint, api.clone());
    let workspace = tempfile::tempdir().unwrap();

    let readiness = block_on(discovery.probe(workspace.path())).unwrap();

    assert!(readiness.desktop_transport_verified);
    assert!(!readiness.desktop_vm_capacity_verified);
    assert_eq!(
        readiness.failure.unwrap().code,
        PrerequisiteCode::InsufficientDisk
    );
    assert_eq!(api.capacity_calls.load(Ordering::SeqCst), 0);
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
fn engine_fingerprint_stays_stable_when_vm_capacity_is_sampled() {
    let endpoint = DockerEndpoint::NamedPipe {
        path: r"\\.\pipe\docker_engine".into(),
        desktop: true,
    };
    let mut before = desktop_snapshot();
    before.docker_storage_accessible = false;
    before.disk_available_bytes = None;
    let general = crate::execution::docker::images::ImageLock::bundled()
        .unwrap()
        .select(
            crate::execution::docker::images::RuntimeProfile::General,
            "amd64",
        )
        .unwrap();
    let unknown =
        crate::execution::docker::engine::approval_fingerprint(&endpoint, &before, &general)
            .unwrap();

    let mut measured = before;
    measured.docker_storage_accessible = true;
    measured.disk_available_bytes = Some(2 * 1024 * 1024 * 1024);
    let known =
        crate::execution::docker::engine::approval_fingerprint(&endpoint, &measured, &general)
            .unwrap();

    assert_eq!(unknown, known);
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

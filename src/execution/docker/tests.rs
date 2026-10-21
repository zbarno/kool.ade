use std::{
    future::Future,
    path::Path,
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};

use super::{
    discovery::{EngineReadiness, PrerequisiteCode, PrerequisiteFailure, validate_snapshot},
    engine::{
        DockerApi, DockerEndpoint, EngineApiTimeouts, EngineDiscovery, EngineFuture, EngineSnapshot,
    },
    images::{ImageObservation, SelectedImage},
};

#[path = "tests/desktop_contract.rs"]
mod desktop_contract;
#[path = "tests/endpoint.rs"]
mod endpoint;
#[path = "tests/images.rs"]
mod images;
#[path = "tests/readiness.rs"]
mod readiness;

fn snapshot() -> EngineSnapshot {
    EngineSnapshot {
        daemon_id: "daemon-test".into(),
        daemon_version: "29.0.0".into(),
        api_version: "1.56".into(),
        minimum_api_version: "1.41".into(),
        os_type: "linux".into(),
        architecture: "amd64".into(),
        security_options: vec![
            "name=rootless".into(),
            "name=seccomp,profile=builtin".into(),
        ],
        rootless: true,
        native_linux: true,
        docker_desktop: false,
        memory_limit: true,
        cpu_quota: true,
        pids_limit: true,
        seccomp_enabled: true,
        docker_storage_accessible: true,
        disk_available_bytes: Some(8 * 1024 * 1024 * 1024),
        minimum_disk_bytes: 2 * 1024 * 1024 * 1024,
    }
}

struct FakeApi {
    inspect_count: AtomicUsize,
    image_count: AtomicUsize,
    image_prepare_count: AtomicUsize,
    inspect_result: Mutex<Result<EngineSnapshot, PrerequisiteFailure>>,
}

struct HangingApi;

impl DockerApi for HangingApi {
    fn inspect<'a>(&'a self) -> EngineFuture<'a, Result<EngineSnapshot, PrerequisiteFailure>> {
        Box::pin(std::future::pending())
    }

    fn image_digests<'a>(&'a self) -> EngineFuture<'a, Result<Vec<String>, PrerequisiteFailure>> {
        Box::pin(std::future::pending())
    }

    fn ensure_image<'a>(
        &'a self,
        _image: &'a SelectedImage,
    ) -> EngineFuture<'a, Result<ImageObservation, PrerequisiteFailure>> {
        Box::pin(std::future::pending())
    }
}

impl DockerApi for FakeApi {
    fn inspect<'a>(&'a self) -> EngineFuture<'a, Result<EngineSnapshot, PrerequisiteFailure>> {
        self.inspect_count.fetch_add(1, Ordering::SeqCst);
        let result = self.inspect_result.lock().unwrap().clone();
        Box::pin(async move { result })
    }

    fn image_digests<'a>(&'a self) -> EngineFuture<'a, Result<Vec<String>, PrerequisiteFailure>> {
        self.image_count.fetch_add(1, Ordering::SeqCst);
        Box::pin(async { Ok(Vec::new()) })
    }

    fn ensure_image<'a>(
        &'a self,
        _image: &'a SelectedImage,
    ) -> EngineFuture<'a, Result<ImageObservation, PrerequisiteFailure>> {
        self.image_prepare_count.fetch_add(1, Ordering::SeqCst);
        Box::pin(async {
            Err(PrerequisiteFailure {
                code: PrerequisiteCode::ImageUnavailable,
                message: "fake image missing".into(),
                retryable: false,
            })
        })
    }
}

#[test]
fn engine_health_calls_are_bounded_when_the_daemon_stops_responding() {
    let discovery = EngineDiscovery::with_api_timeouts(
        DockerEndpoint::Unix {
            path: "/tmp/test.sock".into(),
            desktop: false,
        },
        Arc::new(HangingApi),
        EngineApiTimeouts {
            operation: Duration::from_millis(20),
            image_preparation: Duration::from_millis(20),
        },
    );
    let error = block_on(discovery.probe(Path::new("/tmp"))).unwrap_err();
    assert_eq!(error.code, PrerequisiteCode::DaemonStopped);
    assert!(error.message.contains("timed out"));
}

#[test]
fn trusted_image_preparation_is_bounded_when_registry_access_hangs() {
    let discovery = EngineDiscovery::with_api_timeouts(
        DockerEndpoint::Unix {
            path: "/tmp/test.sock".into(),
            desktop: false,
        },
        Arc::new(HangingApi),
        EngineApiTimeouts {
            operation: Duration::from_secs(1),
            image_preparation: Duration::from_millis(20),
        },
    );
    let image = crate::execution::docker::images::ImageLock::bundled()
        .unwrap()
        .select(
            crate::execution::docker::images::RuntimeProfile::General,
            "amd64",
        )
        .unwrap();
    let error = block_on(discovery.prepare_image(&image)).unwrap_err();
    assert_eq!(error.code, PrerequisiteCode::DaemonStopped);
    assert!(error.message.contains("timed out"));
}

#[test]
fn disconnected_engine_returns_structured_failure_and_does_not_claim_ready() {
    let api = Arc::new(FakeApi {
        inspect_count: AtomicUsize::new(0),
        image_count: AtomicUsize::new(0),
        image_prepare_count: AtomicUsize::new(0),
        inspect_result: Mutex::new(Err(PrerequisiteFailure {
            code: PrerequisiteCode::DaemonStopped,
            message: "daemon disconnected".into(),
            retryable: true,
        })),
    });
    let endpoint = DockerEndpoint::Unix {
        path: "/run/user/1000/docker.sock".into(),
        desktop: false,
    };
    let discovery = EngineDiscovery::with_api(endpoint, api.clone());
    let failure = block_on(discovery.probe(Path::new("/tmp"))).unwrap_err();
    assert_eq!(failure.code, PrerequisiteCode::DaemonStopped);
    assert_eq!(api.image_count.load(Ordering::SeqCst), 0);
}

#[test]
fn engine_probe_cache_is_short_lived_and_explicitly_invalidatable() {
    let mut engine_snapshot = snapshot();
    engine_snapshot.minimum_disk_bytes = 1;
    let api = Arc::new(FakeApi {
        inspect_count: AtomicUsize::new(0),
        image_count: AtomicUsize::new(0),
        image_prepare_count: AtomicUsize::new(0),
        inspect_result: Mutex::new(Ok(engine_snapshot)),
    });
    let discovery = EngineDiscovery::with_api(
        DockerEndpoint::Unix {
            path: "/tmp/test.sock".into(),
            desktop: false,
        },
        api.clone(),
    );
    let first: EngineReadiness = block_on(discovery.probe(Path::new("/tmp"))).unwrap();
    let second = block_on(discovery.probe(Path::new("/tmp"))).unwrap();
    assert!(first.ready && second.ready);
    assert_eq!(api.inspect_count.load(Ordering::SeqCst), 1);
    discovery.invalidate();
    block_on(discovery.probe(Path::new("/tmp"))).unwrap();
    assert_eq!(api.inspect_count.load(Ordering::SeqCst), 2);
}

#[test]
fn engine_health_probe_is_read_only_and_never_prepares_an_image() {
    let mut engine_snapshot = snapshot();
    engine_snapshot.minimum_disk_bytes = 1;
    let api = Arc::new(FakeApi {
        inspect_count: AtomicUsize::new(0),
        image_count: AtomicUsize::new(0),
        image_prepare_count: AtomicUsize::new(0),
        inspect_result: Mutex::new(Ok(engine_snapshot)),
    });
    let discovery = EngineDiscovery::with_api(
        DockerEndpoint::Unix {
            path: "/tmp/test.sock".into(),
            desktop: false,
        },
        api.clone(),
    );

    let readiness = block_on(discovery.probe(Path::new("/tmp"))).unwrap();

    assert!(readiness.ready);
    assert_eq!(api.inspect_count.load(Ordering::SeqCst), 1);
    assert_eq!(api.image_count.load(Ordering::SeqCst), 1);
    assert_eq!(api.image_prepare_count.load(Ordering::SeqCst), 0);
}

#[test]
fn workspace_probe_requires_accessible_staging_before_reporting_ready() {
    let api = Arc::new(FakeApi {
        inspect_count: AtomicUsize::new(0),
        image_count: AtomicUsize::new(0),
        image_prepare_count: AtomicUsize::new(0),
        inspect_result: Mutex::new(Ok(snapshot())),
    });
    let discovery = EngineDiscovery::with_api(
        DockerEndpoint::Unix {
            path: "/tmp/test.sock".into(),
            desktop: false,
        },
        api,
    );
    let readiness = block_on(discovery.probe(Path::new("/path/that/does/not/exist"))).unwrap();
    assert!(!readiness.ready);
    assert!(!readiness.workspace_staging_available);
    assert_eq!(
        readiness.failure.unwrap().code,
        PrerequisiteCode::WorkspaceUnavailable
    );
}

fn block_on<F: Future>(future: F) -> F::Output {
    tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .build()
        .unwrap()
        .block_on(future)
}

#[test]
fn workspace_capacity_never_uses_unverified_paths() {
    let result =
        super::prerequisites::inspect_workspace_capacity(Path::new("/path/that/does/not/exist"));
    assert!(!result.accessible);
    assert!(result.available_bytes.is_none());
}

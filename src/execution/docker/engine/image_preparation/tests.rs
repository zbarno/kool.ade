use std::sync::{
    Arc, Mutex,
    atomic::{AtomicUsize, Ordering},
};

use crate::execution::docker::{
    discovery::{PrerequisiteCode, PrerequisiteFailure},
    engine::{
        DockerApi, DockerEndpoint, EngineDiscovery, EngineFuture, EngineSnapshot, ImagePullControl,
    },
    images::{ImageLock, ImageObservation, RuntimeProfile, SelectedImage},
};

use crate::app::bootstrap_approval::ApprovedAction;

#[derive(Clone, Copy)]
enum PullBehavior {
    Progress,
    WaitForCancellation,
}

struct FakePullApi {
    snapshot: Mutex<EngineSnapshot>,
    pulls: AtomicUsize,
    behavior: PullBehavior,
}

impl DockerApi for FakePullApi {
    fn inspect<'a>(&'a self) -> EngineFuture<'a, Result<EngineSnapshot, PrerequisiteFailure>> {
        Box::pin(async { Ok(self.snapshot.lock().unwrap().clone()) })
    }

    fn image_digests<'a>(&'a self) -> EngineFuture<'a, Result<Vec<String>, PrerequisiteFailure>> {
        Box::pin(async { Ok(Vec::new()) })
    }

    fn inspect_image<'a>(
        &'a self,
        _image: &'a SelectedImage,
    ) -> EngineFuture<'a, Result<ImageObservation, PrerequisiteFailure>> {
        Box::pin(async {
            Err(super::super::failure(
                PrerequisiteCode::ImageUnavailable,
                "synthetic image is not installed",
                false,
            ))
        })
    }

    fn ensure_image<'a>(
        &'a self,
        image: &'a SelectedImage,
        _expected_engine_fingerprint: Option<&'a str>,
    ) -> EngineFuture<'a, Result<ImageObservation, PrerequisiteFailure>> {
        Box::pin(async move { Ok(observation(image)) })
    }

    fn ensure_image_with_control<'a>(
        &'a self,
        image: &'a SelectedImage,
        _expected_engine_fingerprint: &'a str,
        _verified_capacity_bytes: Option<u64>,
        control: &'a ImagePullControl,
    ) -> EngineFuture<'a, Result<ImageObservation, PrerequisiteFailure>> {
        Box::pin(async move {
            self.pulls.fetch_add(1, Ordering::AcqRel);
            match self.behavior {
                PullBehavior::Progress => {
                    for index in 0..512 {
                        control.record_progress(
                            Some(&format!("layer-{}", index % 8)),
                            Some(index % 101),
                            Some(100),
                        )?;
                    }
                    Ok(observation(image))
                }
                PullBehavior::WaitForCancellation => loop {
                    if control.is_cancelled() {
                        return Err(super::super::failure(
                            PrerequisiteCode::ImageUnavailable,
                            "fake pull stopped after cancellation",
                            true,
                        ));
                    }
                    tokio::time::sleep(std::time::Duration::from_millis(5)).await;
                },
            }
        })
    }
}

#[path = "tests/authorization.rs"]
mod authorization_tests;
#[path = "tests/progress.rs"]
mod progress_tests;
#[path = "tests/storage.rs"]
mod storage_tests;

fn engine(api: Arc<FakePullApi>) -> EngineDiscovery {
    EngineDiscovery::with_api(
        DockerEndpoint::Unix {
            path: "/tmp/synthetic-docker.sock".into(),
            desktop: false,
        },
        api,
    )
}

fn fake_api(daemon_id: &str, behavior: PullBehavior) -> FakePullApi {
    FakePullApi {
        snapshot: Mutex::new(snapshot(daemon_id)),
        pulls: AtomicUsize::new(0),
        behavior,
    }
}

fn snapshot(daemon_id: &str) -> EngineSnapshot {
    EngineSnapshot {
        daemon_id: daemon_id.into(),
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
    }
}

fn locked_image() -> SelectedImage {
    ImageLock::bundled()
        .unwrap()
        .select(RuntimeProfile::General, "amd64")
        .unwrap()
}

fn observation(image: &SelectedImage) -> ImageObservation {
    ImageObservation {
        image_id: image.digest.clone(),
        repo_digests: vec![image.reference.clone()],
        os: "linux".into(),
        architecture: "amd64".into(),
        configured_user: "10001:10001".into(),
        labels: Default::default(),
    }
}

fn authorization(
    engine: &EngineDiscovery,
    image: &SelectedImage,
    daemon_id: &str,
) -> ApprovedAction {
    let snapshot = snapshot(daemon_id);
    let engine_fingerprint =
        super::super::approval_fingerprint(engine.endpoint(), &snapshot, image).unwrap();
    let engine_identity = crate::app::bootstrap_approval::engine_identity_fingerprint(
        &engine.endpoint().fingerprint(),
        daemon_id,
    )
    .unwrap();
    let profile_name = match image.profile {
        RuntimeProfile::General => "general",
        RuntimeProfile::Rust => "rust",
        RuntimeProfile::Node => "node",
        RuntimeProfile::Python => "python",
        RuntimeProfile::Go => "go",
        RuntimeProfile::BrowserQa => "browser_qa",
        RuntimeProfile::DesktopQa => "desktop_qa",
    };
    ApprovedAction::for_test(
        profile_name,
        &format!("setup:images:{profile_name}:amd64:{engine_identity}"),
        image,
        &engine_fingerprint,
        &engine_identity,
    )
}

fn runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
}

//! Production boundary: the approved image preparation path runs through readiness,
//! consent binding, image pull, guest-capacity measurement, and smoke verification.
//! A helper-only test cannot prove these gates are ordered and connected. The API is
//! synthetic and staging is a temporary local directory; no Docker, network, or SSH
//! is required. This does not certify native Desktop transport or host staging.

use super::*;
use std::sync::atomic::AtomicBool;

struct DesktopBootstrapApi {
    image: SelectedImage,
    image_ready: AtomicBool,
    pull_count: AtomicUsize,
    capacity_count: AtomicUsize,
    smoke_count: AtomicUsize,
    events: Mutex<Vec<&'static str>>,
}

impl DesktopBootstrapApi {
    fn observation(&self) -> ImageObservation {
        ImageObservation {
            image_id: self.image.digest.clone(),
            repo_digests: vec![self.image.reference.clone()],
            os: "linux".into(),
            architecture: "amd64".into(),
            configured_user: "10001:10001".into(),
            labels: Default::default(),
        }
    }

    fn snapshot(&self) -> EngineSnapshot {
        EngineSnapshot {
            daemon_id: "desktop-daemon".into(),
            daemon_version: "28.0.0".into(),
            api_version: "1.53".into(),
            minimum_api_version: "1.41".into(),
            os_type: "linux".into(),
            architecture: "amd64".into(),
            security_options: vec!["name=seccomp".into()],
            rootless: false,
            native_linux: false,
            docker_desktop: true,
            memory_limit: true,
            cpu_quota: true,
            pids_limit: true,
            cgroup_version: Some("2".into()),
            cgroup_driver: Some("cgroupfs".into()),
            seccomp_enabled: true,
            docker_storage_accessible: false,
            disk_available_bytes: None,
            minimum_disk_bytes: 1024,
        }
    }
}

impl DockerApi for DesktopBootstrapApi {
    fn inspect<'a>(&'a self) -> EngineFuture<'a, Result<EngineSnapshot, PrerequisiteFailure>> {
        Box::pin(async move { Ok(self.snapshot()) })
    }

    fn image_digests<'a>(&'a self) -> EngineFuture<'a, Result<Vec<String>, PrerequisiteFailure>> {
        Box::pin(async move {
            Ok(if self.image_ready.load(Ordering::Acquire) {
                vec![self.image.reference.clone()]
            } else {
                Vec::new()
            })
        })
    }

    fn inspect_image<'a>(
        &'a self,
        _image: &'a SelectedImage,
    ) -> EngineFuture<'a, Result<ImageObservation, PrerequisiteFailure>> {
        Box::pin(async move {
            if self.image_ready.load(Ordering::Acquire) {
                Ok(self.observation())
            } else {
                Err(failure(
                    PrerequisiteCode::ImageUnavailable,
                    "synthetic locked image is absent",
                    false,
                ))
            }
        })
    }

    fn desktop_storage_capacity<'a>(
        &'a self,
        image: &'a SelectedImage,
        _expected_engine_fingerprint: &'a str,
    ) -> EngineFuture<'a, Result<u64, PrerequisiteFailure>> {
        Box::pin(async move {
            assert_eq!(image, &self.image);
            assert!(self.image_ready.load(Ordering::Acquire));
            self.capacity_count.fetch_add(1, Ordering::AcqRel);
            self.events.lock().unwrap().push("capacity");
            Ok(8 * 1024 * 1024 * 1024)
        })
    }

    #[cfg(test)]
    fn ensure_image<'a>(
        &'a self,
        image: &'a SelectedImage,
        _expected_engine_fingerprint: Option<&'a str>,
    ) -> EngineFuture<'a, Result<ImageObservation, PrerequisiteFailure>> {
        Box::pin(async move {
            assert_eq!(image, &self.image);
            self.image_ready.store(true, Ordering::Release);
            Ok(self.observation())
        })
    }

    fn ensure_image_with_control<'a>(
        &'a self,
        image: &'a SelectedImage,
        _expected_engine_fingerprint: &'a str,
        verified_capacity_bytes: Option<u64>,
        control: &'a ImagePullControl,
    ) -> EngineFuture<'a, Result<ImageObservation, PrerequisiteFailure>> {
        Box::pin(async move {
            assert_eq!(image, &self.image);
            assert_eq!(verified_capacity_bytes, None);
            if control.is_cancelled() {
                return Err(failure(
                    PrerequisiteCode::ImageUnavailable,
                    "synthetic image pull cancelled",
                    true,
                ));
            }
            self.image_ready.store(true, Ordering::Release);
            self.pull_count.fetch_add(1, Ordering::AcqRel);
            self.events.lock().unwrap().push("pull");
            Ok(self.observation())
        })
    }

    fn smoke_test<'a>(
        &'a self,
        image: &'a SelectedImage,
        _container_name: &'a str,
        _expected_engine_fingerprint: Option<&'a str>,
    ) -> EngineFuture<'a, Result<(), PrerequisiteFailure>> {
        Box::pin(async move {
            assert_eq!(image, &self.image);
            self.smoke_count.fetch_add(1, Ordering::AcqRel);
            self.events.lock().unwrap().push("smoke");
            Ok(())
        })
    }
}

#[test]
fn desktop_general_capacity_bootstrap_is_measured_after_the_bounded_pull() {
    let image = ImageLock::bundled()
        .unwrap()
        .select(RuntimeProfile::General, "amd64")
        .unwrap();
    let api = Arc::new(DesktopBootstrapApi {
        image: image.clone(),
        image_ready: AtomicBool::new(false),
        pull_count: AtomicUsize::new(0),
        capacity_count: AtomicUsize::new(0),
        smoke_count: AtomicUsize::new(0),
        events: Mutex::new(Vec::new()),
    });
    let endpoint = DockerEndpoint::NamedPipe {
        path: r"\\.\pipe\docker_engine".into(),
        desktop: true,
    };
    let engine = EngineDiscovery::with_test_verified_desktop_transport(endpoint, api.clone());
    let staging = tempfile::tempdir().unwrap();
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let before_pull = runtime
        .block_on(engine.inspect_snapshot_for_image_preparation(RuntimeProfile::General))
        .unwrap();
    let engine_fingerprint = approval_fingerprint(engine.endpoint(), &before_pull, &image).unwrap();
    let engine_identity = crate::app::bootstrap_approval::engine_identity_fingerprint(
        &engine.endpoint().fingerprint(),
        &before_pull.daemon_id,
    )
    .unwrap();
    let authorization =
        crate::app::bootstrap_approval::ApprovedAction::for_test_with_capacity_probe(
            "general",
            &format!("setup:images:general:amd64:{engine_identity}"),
            &image,
            &engine_fingerprint,
            &engine_identity,
            true,
        );

    let receipt = runtime
        .block_on(engine.test_approved_image(staging.path(), authorization, &pull_control()))
        .unwrap();

    assert_eq!(receipt.image_digest, image.digest);
    assert_eq!(receipt.engine_identity_fingerprint, engine_identity);
    assert_eq!(api.pull_count.load(Ordering::Acquire), 1);
    assert_eq!(api.capacity_count.load(Ordering::Acquire), 1);
    assert_eq!(api.smoke_count.load(Ordering::Acquire), 1);
    assert_eq!(*api.events.lock().unwrap(), ["pull", "capacity", "smoke"]);
}

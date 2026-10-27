use std::sync::{
    Arc, Mutex,
    atomic::{AtomicUsize, Ordering},
};

use crate::execution::docker::{
    discovery::{PrerequisiteCode, PrerequisiteFailure},
    engine::{DockerApi, DockerEndpoint, EngineDiscovery, EngineFuture, EngineSnapshot},
    images::{ImageLock, ImageObservation, RuntimeProfile, SelectedImage},
};

struct InspectApi {
    observed: Mutex<Option<ImageObservation>>,
    derived_observed: Mutex<Option<ImageObservation>>,
    inspect_count: AtomicUsize,
    derived_inspect_count: AtomicUsize,
    pull_count: AtomicUsize,
}

impl DockerApi for InspectApi {
    fn inspect<'a>(&'a self) -> EngineFuture<'a, Result<EngineSnapshot, PrerequisiteFailure>> {
        Box::pin(async {
            Ok(EngineSnapshot {
                daemon_id: "synthetic-daemon".into(),
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
        Box::pin(async { Ok(Vec::new()) })
    }

    fn inspect_image<'a>(
        &'a self,
        _image: &'a SelectedImage,
    ) -> EngineFuture<'a, Result<ImageObservation, PrerequisiteFailure>> {
        Box::pin(async move {
            self.inspect_count.fetch_add(1, Ordering::Relaxed);
            self.observed.lock().unwrap().clone().ok_or_else(|| {
                super::super::failure(
                    PrerequisiteCode::ImageUnavailable,
                    "synthetic image missing",
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
            Ok(observation(image))
        })
    }

    fn inspect_derived_image<'a>(
        &'a self,
        _image: &'a SelectedImage,
    ) -> EngineFuture<'a, Result<ImageObservation, PrerequisiteFailure>> {
        Box::pin(async move {
            self.derived_inspect_count.fetch_add(1, Ordering::Relaxed);
            self.derived_observed
                .lock()
                .unwrap()
                .clone()
                .ok_or_else(|| {
                    super::super::failure(
                        PrerequisiteCode::ImageUnavailable,
                        "synthetic derived image missing",
                        false,
                    )
                })
        })
    }
}

#[test]
fn missing_locked_image_is_inspected_and_never_pulled() {
    let image = locked_general_image();
    let api = Arc::new(api(None, None));
    let engine = engine(api.clone());
    let error = runtime()
        .block_on(engine.verify_image_present(&image))
        .unwrap_err();

    assert_eq!(error.code, PrerequisiteCode::ImageUnavailable);
    assert_eq!(api.inspect_count.load(Ordering::Relaxed), 1);
    assert_eq!(api.pull_count.load(Ordering::Relaxed), 0);
}

#[test]
fn present_locked_image_is_verified_without_pull() {
    let image = locked_general_image();
    let api = Arc::new(api(Some(observation(&image)), None));
    let engine = engine(api.clone());

    runtime()
        .block_on(engine.verify_image_present(&image))
        .unwrap();

    assert_eq!(api.inspect_count.load(Ordering::Relaxed), 1);
    assert_eq!(api.pull_count.load(Ordering::Relaxed), 0);
}

#[test]
fn registered_derived_image_still_uses_local_inspection_without_pull() {
    let content_identity = digest('2');
    let base_digest = digest('3');
    let image = SelectedImage {
        profile: RuntimeProfile::General,
        platform: "linux/amd64".into(),
        reference: digest('1'),
        digest: digest('1'),
        runtime_user: "10001:10001".into(),
        tool_versions: Default::default(),
    };
    let mut observed = ImageObservation {
        image_id: image.digest.clone(),
        repo_digests: Vec::new(),
        os: "linux".into(),
        architecture: "amd64".into(),
        configured_user: image.runtime_user.clone(),
        labels: Default::default(),
    };
    observed
        .labels
        .insert("io.koolade.content".into(), content_identity.clone());
    observed
        .labels
        .insert("io.koolade.base-digest".into(), base_digest.clone());
    let api = Arc::new(api(None, Some(observed)));
    let engine = engine(api.clone());

    runtime()
        .block_on(engine.prepare_derived_image(&image, &content_identity, &base_digest))
        .unwrap();

    assert_eq!(api.derived_inspect_count.load(Ordering::Relaxed), 1);
    assert_eq!(api.pull_count.load(Ordering::Relaxed), 0);
}

fn engine(api: Arc<InspectApi>) -> EngineDiscovery {
    EngineDiscovery::with_api(
        DockerEndpoint::Unix {
            path: "/tmp/synthetic-docker.sock".into(),
            desktop: false,
        },
        api,
    )
}

fn api(
    observed: Option<ImageObservation>,
    derived_observed: Option<ImageObservation>,
) -> InspectApi {
    InspectApi {
        observed: Mutex::new(observed),
        derived_observed: Mutex::new(derived_observed),
        inspect_count: AtomicUsize::new(0),
        derived_inspect_count: AtomicUsize::new(0),
        pull_count: AtomicUsize::new(0),
    }
}

fn locked_general_image() -> SelectedImage {
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
        architecture: image.platform.strip_prefix("linux/").unwrap().into(),
        configured_user: image.runtime_user.clone(),
        labels: Default::default(),
    }
}

fn digest(hex: char) -> String {
    format!("sha256:{}", hex.to_string().repeat(64))
}

fn runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
}

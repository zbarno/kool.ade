use super::*;

#[test]
fn environment_test_uses_general_locked_image_and_scoped_smoke_name() {
    let general = ImageLock::bundled()
        .unwrap()
        .select(RuntimeProfile::General, "amd64")
        .unwrap();
    let api = std::sync::Arc::new(SmokeApi {
        smoke_image: Mutex::new(None),
        smoke_name: Mutex::new(None),
        derived_observation: Mutex::new(None),
        daemon_id: Mutex::new("fake-daemon".into()),
        pull_count: AtomicUsize::new(0),
        swap_after_pull: Mutex::new(None),
        swap_after_smoke: Mutex::new(None),
        available_images: Mutex::new(vec![general.reference]),
        tamper_after_smoke: false,
        cancel_after_smoke: Mutex::new(None),
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
    assert_eq!(api.pull_count.load(Ordering::Relaxed), 0);
}

#[test]
fn environment_test_does_not_pull_a_missing_general_image() {
    let api = std::sync::Arc::new(SmokeApi {
        smoke_image: Mutex::new(None),
        smoke_name: Mutex::new(None),
        derived_observation: Mutex::new(None),
        daemon_id: Mutex::new("fake-daemon".into()),
        pull_count: AtomicUsize::new(0),
        swap_after_pull: Mutex::new(None),
        swap_after_smoke: Mutex::new(None),
        available_images: Mutex::new(Vec::new()),
        tamper_after_smoke: false,
        cancel_after_smoke: Mutex::new(None),
    });
    let engine = EngineDiscovery::with_api(
        DockerEndpoint::Unix {
            path: "/tmp/fake-docker.sock".into(),
            desktop: false,
        },
        api.clone(),
    );
    let staging = tempfile::tempdir().unwrap();
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();

    let error = runtime
        .block_on(engine.test_environment(staging.path()))
        .unwrap_err();
    assert_eq!(error.code, PrerequisiteCode::ImageUnavailable);
    assert_eq!(api.pull_count.load(Ordering::Relaxed), 0);
    assert!(api.smoke_image.lock().unwrap().is_none());
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
        daemon_id: Mutex::new("fake-daemon".into()),
        pull_count: AtomicUsize::new(0),
        swap_after_pull: Mutex::new(None),
        swap_after_smoke: Mutex::new(None),
        available_images: Mutex::new(Vec::new()),
        tamper_after_smoke: false,
        cancel_after_smoke: Mutex::new(None),
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

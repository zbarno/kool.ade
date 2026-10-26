use super::*;

#[test]
fn digest_change_after_smoke_fails_without_a_preparation_receipt() {
    let api = std::sync::Arc::new(SmokeApi {
        smoke_image: Mutex::new(None),
        smoke_name: Mutex::new(None),
        derived_observation: Mutex::new(None),
        daemon_id: Mutex::new("approved-daemon".into()),
        pull_count: AtomicUsize::new(0),
        swap_after_pull: Mutex::new(None),
        swap_after_smoke: Mutex::new(None),
        available_images: Mutex::new(Vec::new()),
        tamper_after_smoke: true,
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
    let snapshot = runtime.block_on(engine.inspect_snapshot()).unwrap();
    let general = ImageLock::bundled()
        .unwrap()
        .select(RuntimeProfile::General, "amd64")
        .unwrap();
    let fingerprint = approval_fingerprint(engine.endpoint(), &snapshot, &general).unwrap();
    let engine_identity = crate::app::bootstrap_approval::engine_identity_fingerprint(
        &engine.endpoint().fingerprint(),
        &snapshot.daemon_id,
    )
    .unwrap();
    let approval = crate::app::bootstrap_approval::ApprovedAction::for_test(
        "general",
        &format!("setup:images:general:amd64:{engine_identity}"),
        &general,
        &fingerprint,
        &engine_identity,
    );

    let error = runtime
        .block_on(engine.test_approved_image(staging.path(), approval, &pull_control()))
        .unwrap_err();
    assert_eq!(error.code, PrerequisiteCode::ImageDigestMismatch);
    assert_eq!(api.pull_count.load(Ordering::Relaxed), 1);
    assert_eq!(
        api.smoke_image.lock().unwrap().as_deref(),
        Some(general.reference.as_str())
    );
}

#[test]
fn docker_hub_short_alias_remains_valid_in_the_final_image_inventory() {
    let general = ImageLock::bundled()
        .unwrap()
        .select(RuntimeProfile::General, "amd64")
        .unwrap();
    let docker_hub_alias = general.reference.replace("docker.io/library/", "");
    let api = std::sync::Arc::new(SmokeApi {
        smoke_image: Mutex::new(None),
        smoke_name: Mutex::new(None),
        derived_observation: Mutex::new(None),
        daemon_id: Mutex::new("approved-daemon".into()),
        pull_count: AtomicUsize::new(0),
        swap_after_pull: Mutex::new(None),
        swap_after_smoke: Mutex::new(None),
        available_images: Mutex::new(vec![docker_hub_alias.clone()]),
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
    let snapshot = runtime.block_on(engine.inspect_snapshot()).unwrap();
    let fingerprint = approval_fingerprint(engine.endpoint(), &snapshot, &general).unwrap();
    let engine_identity = crate::app::bootstrap_approval::engine_identity_fingerprint(
        &engine.endpoint().fingerprint(),
        &snapshot.daemon_id,
    )
    .unwrap();
    let approval = crate::app::bootstrap_approval::ApprovedAction::for_test(
        "general",
        &format!("setup:images:general:amd64:{engine_identity}"),
        &general,
        &fingerprint,
        &engine_identity,
    );

    let receipt = runtime
        .block_on(engine.test_approved_image(staging.path(), approval, &pull_control()))
        .unwrap();

    assert_eq!(receipt.image_digest, general.digest);
    assert_eq!(
        api.available_images.lock().unwrap().as_slice(),
        &[docker_hub_alias]
    );
    assert_eq!(api.pull_count.load(Ordering::Relaxed), 1);
}

#[test]
fn engine_identity_change_during_smoke_fails_without_a_preparation_receipt() {
    let api = std::sync::Arc::new(SmokeApi {
        smoke_image: Mutex::new(None),
        smoke_name: Mutex::new(None),
        derived_observation: Mutex::new(None),
        daemon_id: Mutex::new("approved-daemon".into()),
        pull_count: AtomicUsize::new(0),
        swap_after_pull: Mutex::new(None),
        swap_after_smoke: Mutex::new(Some("replacement-daemon".into())),
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
    let snapshot = runtime.block_on(engine.inspect_snapshot()).unwrap();
    let general = ImageLock::bundled()
        .unwrap()
        .select(RuntimeProfile::General, "amd64")
        .unwrap();
    let fingerprint = approval_fingerprint(engine.endpoint(), &snapshot, &general).unwrap();
    let engine_identity = crate::app::bootstrap_approval::engine_identity_fingerprint(
        &engine.endpoint().fingerprint(),
        &snapshot.daemon_id,
    )
    .unwrap();
    let approval = crate::app::bootstrap_approval::ApprovedAction::for_test(
        "general",
        &format!("setup:images:general:amd64:{engine_identity}"),
        &general,
        &fingerprint,
        &engine_identity,
    );

    let error = runtime
        .block_on(engine.test_approved_image(staging.path(), approval, &pull_control()))
        .unwrap_err();
    assert_eq!(error.code, PrerequisiteCode::UntrustedEndpoint);
    assert_eq!(api.pull_count.load(Ordering::Relaxed), 1);
    assert!(api.smoke_image.lock().unwrap().is_some());
}

#[test]
fn cancellation_during_smoke_prevents_a_preparation_receipt() {
    let control = pull_control();
    let api = std::sync::Arc::new(SmokeApi {
        smoke_image: Mutex::new(None),
        smoke_name: Mutex::new(None),
        derived_observation: Mutex::new(None),
        daemon_id: Mutex::new("approved-daemon".into()),
        pull_count: AtomicUsize::new(0),
        swap_after_pull: Mutex::new(None),
        swap_after_smoke: Mutex::new(None),
        available_images: Mutex::new(Vec::new()),
        tamper_after_smoke: false,
        cancel_after_smoke: Mutex::new(Some(control.clone())),
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
    let snapshot = runtime.block_on(engine.inspect_snapshot()).unwrap();
    let general = ImageLock::bundled()
        .unwrap()
        .select(RuntimeProfile::General, "amd64")
        .unwrap();
    let fingerprint = approval_fingerprint(engine.endpoint(), &snapshot, &general).unwrap();
    let engine_identity = crate::app::bootstrap_approval::engine_identity_fingerprint(
        &engine.endpoint().fingerprint(),
        &snapshot.daemon_id,
    )
    .unwrap();
    let approval = crate::app::bootstrap_approval::ApprovedAction::for_test(
        "general",
        &format!("setup:images:general:amd64:{engine_identity}"),
        &general,
        &fingerprint,
        &engine_identity,
    );

    let error = runtime
        .block_on(engine.test_approved_image(staging.path(), approval, &control))
        .unwrap_err();
    assert_eq!(error.code, PrerequisiteCode::ImageUnavailable);
    assert_eq!(api.pull_count.load(Ordering::Relaxed), 1);
    assert!(api.smoke_image.lock().unwrap().is_some());
}

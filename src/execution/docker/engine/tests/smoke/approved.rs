use super::*;

#[path = "approved/receipts.rs"]
mod receipts;

#[test]
fn approved_image_uses_the_exact_lock_entry_on_the_bound_engine() {
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
    let receipt = runtime
        .block_on(engine.test_approved_image(staging.path(), approval, &pull_control()))
        .unwrap();
    assert_eq!(receipt.profile, RuntimeProfile::General);
    assert_eq!(receipt.architecture, "amd64");
    assert_eq!(receipt.image_digest, general.digest);
    assert_eq!(receipt.engine_identity_fingerprint, engine_identity);
    assert_eq!(
        api.smoke_image.lock().unwrap().as_deref(),
        Some(general.reference.as_str())
    );
    assert_eq!(api.pull_count.load(Ordering::Relaxed), 1);
}

#[test]
fn changed_daemon_identity_blocks_the_pull_before_any_side_effect() {
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
    *api.daemon_id.lock().unwrap() = "restarted-daemon".into();

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
    assert_eq!(api.pull_count.load(Ordering::Relaxed), 0);
    assert!(api.smoke_image.lock().unwrap().is_none());
}

#[test]
fn daemon_swap_after_pull_blocks_the_smoke_side_effect() {
    let api = std::sync::Arc::new(SmokeApi {
        smoke_image: Mutex::new(None),
        smoke_name: Mutex::new(None),
        derived_observation: Mutex::new(None),
        daemon_id: Mutex::new("approved-daemon".into()),
        pull_count: AtomicUsize::new(0),
        swap_after_pull: Mutex::new(Some("replacement-daemon".into())),
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
    assert!(api.smoke_image.lock().unwrap().is_none());
}

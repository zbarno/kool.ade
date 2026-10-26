use super::*;

#[path = "receipts/receipt_failures.rs"]
mod receipt_failures;

#[test]
fn approved_rust_image_receipt_matches_its_exact_lock_entry_and_engine() {
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
    let rust = ImageLock::bundled()
        .unwrap()
        .select(RuntimeProfile::Rust, "amd64")
        .unwrap();
    let fingerprint = approval_fingerprint(engine.endpoint(), &snapshot, &rust).unwrap();
    let engine_identity = crate::app::bootstrap_approval::engine_identity_fingerprint(
        &engine.endpoint().fingerprint(),
        &snapshot.daemon_id,
    )
    .unwrap();
    let approval = crate::app::bootstrap_approval::ApprovedAction::for_test(
        "rust",
        &format!("setup:images:rust:amd64:{engine_identity}"),
        &rust,
        &fingerprint,
        &engine_identity,
    );

    let receipt = runtime
        .block_on(engine.test_approved_image(staging.path(), approval, &pull_control()))
        .unwrap();
    assert_eq!(receipt.profile, RuntimeProfile::Rust);
    assert_eq!(receipt.architecture, "amd64");
    assert_eq!(receipt.image_digest, rust.digest);
    assert_eq!(receipt.engine_identity_fingerprint, engine_identity);
    assert_eq!(
        api.smoke_image.lock().unwrap().as_deref(),
        Some(rust.reference.as_str())
    );
    assert_eq!(api.pull_count.load(Ordering::Relaxed), 1);
}

#[test]
fn disabled_qa_profile_cannot_be_prepared_even_with_a_forged_test_capability() {
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
    let general = ImageLock::bundled()
        .unwrap()
        .select(RuntimeProfile::General, "amd64")
        .unwrap();
    let approval = crate::app::bootstrap_approval::ApprovedAction::for_test(
        "browser_qa",
        &format!("setup:images:browser_qa:amd64:{}", "e".repeat(64)),
        &general,
        &"a".repeat(64),
        &"e".repeat(64),
    );
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();

    let error = runtime
        .block_on(engine.test_approved_image(Path::new("/unused"), approval, &pull_control()))
        .unwrap_err();
    assert_eq!(error.code, PrerequisiteCode::ProfileUnsupported);
    assert_eq!(api.pull_count.load(Ordering::Relaxed), 0);
    assert!(api.smoke_image.lock().unwrap().is_none());
}

#[test]
fn missing_malformed_or_mismatched_engine_identity_suffix_blocks_before_pull() {
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
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let snapshot = runtime.block_on(engine.inspect_snapshot()).unwrap();
    let general = ImageLock::bundled()
        .unwrap()
        .select(RuntimeProfile::General, "amd64")
        .unwrap();
    let approval_fingerprint =
        approval_fingerprint(engine.endpoint(), &snapshot, &general).unwrap();
    let engine_identity = crate::app::bootstrap_approval::engine_identity_fingerprint(
        &engine.endpoint().fingerprint(),
        &snapshot.daemon_id,
    )
    .unwrap();

    for task_id in [
        "setup:images:general:amd64".to_owned(),
        format!("setup:images:general:amd64:{}", "g".repeat(64)),
        format!("setup:images:general:amd64:{}", "f".repeat(64)),
    ] {
        let approval = crate::app::bootstrap_approval::ApprovedAction::for_test(
            "general",
            &task_id,
            &general,
            &approval_fingerprint,
            &engine_identity,
        );
        let error = runtime
            .block_on(engine.test_approved_image(Path::new("/unused"), approval, &pull_control()))
            .unwrap_err();
        assert_eq!(error.code, PrerequisiteCode::UntrustedEndpoint);
    }

    assert_eq!(api.pull_count.load(Ordering::Relaxed), 0);
    assert!(api.smoke_image.lock().unwrap().is_none());
}

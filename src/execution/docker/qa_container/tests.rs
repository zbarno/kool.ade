use super::*;

#[path = "tests/browser_rootless.rs"]
mod browser_rootless;
#[path = "tests/desktop_rootless.rs"]
mod desktop_rootless;

#[path = "tests/artifact_permissions.rs"]
mod artifact_permissions;
#[path = "tests/cache_architecture.rs"]
mod cache_architecture;
#[path = "tests/driver_policy.rs"]
mod driver_policy;

fn request() -> crate::runtime_qa::QaRunRequestV1 {
    let uid = || uuid::Uuid::new_v4().hyphenated().to_string();
    crate::runtime_qa::QaRunRequestV1 {
        schema_version: 1,
        qa_run_id: uid(),
        parent_task_uid: uid(),
        target_id: Some("fixture-target".into()),
        planning_item_uid: None,
        feature_uid: uid(),
        code_head_sha: "a".repeat(64),
        build_artifact_sha256: "b".repeat(64),
        complete_diff_hash: "c".repeat(64),
        review_subject_sha256: "f".repeat(64),
        approved_spec_hash: "d".repeat(64),
        requirement_ids: vec!["REQ-1".into()],
        acceptance_criteria: vec!["The application runs.".into()],
        relevant_platforms: vec![crate::runtime_qa::QaPlatform::Web],
        startup_recipe_ref: "recipe-v1".into(),
        startup_recipe_sha256: "e".repeat(64),
        execution_session_id: uid(),
        driver_profile_version: "browser-1".into(),
        test_data_fixture_ref: None,
        dependency_cache_refs: Vec::new(),
        bounded_limits: crate::runtime_qa::QaBoundedLimitsV1 {
            max_scenarios: 1,
            max_actions_per_scenario: 8,
            max_total_actions: 8,
            timeout_seconds: 30,
            max_evidence_bytes: 1024 * 1024,
            max_screenshots: 2,
        },
        author_run_id: uid(),
        author_session_id: uid(),
    }
}

fn resources() -> ResourceBudgetV1 {
    ResourceBudgetV1 {
        cpu_millis: 2000,
        memory_bytes: 1024 * 1024 * 1024,
        process_count: 64,
        temporary_disk_bytes: 1024 * 1024 * 1024,
        output_bytes: 1024 * 1024,
    }
}

fn selected_qa_image(profile: RuntimeProfile) -> SelectedImage {
    let repository = match profile {
        RuntimeProfile::BrowserQa => "ghcr.io/zbarno/koolade-runtime-qa-browser",
        RuntimeProfile::DesktopQa => "ghcr.io/zbarno/koolade-runtime-qa-desktop",
        _ => panic!("expected a QA profile"),
    };
    let digest = format!("sha256:{}", "a".repeat(64));
    let tool_versions = match profile {
        RuntimeProfile::BrowserQa => std::collections::BTreeMap::from([
            ("node".into(), "24.21.0".into()),
            ("npm".into(), "11.19.0".into()),
            ("playwright".into(), "1.57.0".into()),
            ("chromium".into(), "fixture".into()),
            (
                "driver".into(),
                "koolade-runtime-qa-browser-driver-1.0.0".into(),
            ),
            ("base".into(), "fixture".into()),
        ]),
        RuntimeProfile::DesktopQa => std::collections::BTreeMap::from([
            ("driver".into(), "linux-x11-driver/v1".into()),
            ("xvfb".into(), "fixture".into()),
            ("base".into(), "fixture".into()),
        ]),
        _ => unreachable!(),
    };
    SelectedImage {
        profile,
        platform: "linux/amd64".into(),
        reference: format!("{repository}@{digest}"),
        digest,
        runtime_user: "10001:10001".into(),
        tool_versions,
    }
}

#[test]
fn selected_artifact_snapshot_contains_only_receipt_verified_files() {
    use sha2::{Digest, Sha256};
    use std::collections::BTreeMap;

    let root = std::env::temp_dir().join(format!("qa-selected-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(root.join("dist")).unwrap();
    std::fs::write(root.join("dist/index.html"), b"approved build").unwrap();
    std::fs::write(root.join("secret.txt"), b"must not be uploaded").unwrap();
    let hashes = BTreeMap::from([(
        "dist/index.html".to_owned(),
        format!("{:x}", Sha256::digest(b"approved build")),
    )]);

    let (upload, digest) =
        snapshot_selected_files(&root, &["dist/index.html".into()], &hashes, 1024).unwrap();
    assert_eq!(upload.target, "/workspace");
    assert_eq!(digest.len(), 64);
    let mut archive = tar::Archive::new(upload.archive.as_slice());
    let names = archive
        .entries()
        .unwrap()
        .map(|entry| {
            entry
                .unwrap()
                .path()
                .unwrap()
                .to_string_lossy()
                .into_owned()
        })
        .collect::<Vec<_>>();
    assert_eq!(names, vec!["dist/", "dist/index.html"]);

    let stale_hashes = BTreeMap::from([("dist/index.html".to_owned(), "0".repeat(64))]);
    assert!(
        snapshot_selected_files(&root, &["dist/index.html".into()], &stale_hashes, 1024,).is_err()
    );
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn artifact_mount_rejects_host_root_and_arbitrary_container_paths() {
    let root = std::env::temp_dir();
    assert!(QaArtifactMount::inspect(&root, "/etc").is_err());
    assert!(QaArtifactMount::inspect(&root, "/workspace/../../tmp").is_err());
}

#[test]
fn artifact_mount_rejects_symlink_sources() {
    let root = std::env::temp_dir().join(format!("qa-mount-{}", uuid::Uuid::new_v4()));
    let destination = root.with_extension("destination");
    std::fs::create_dir_all(&destination).unwrap();
    #[cfg(unix)]
    std::os::unix::fs::symlink(&destination, &root).unwrap();
    let result = QaArtifactMount::inspect(&root, "/workspace");
    assert!(result.is_err());
    let _ = std::fs::remove_file(&root);
    let _ = std::fs::remove_dir_all(destination);
}

#[test]
fn container_spec_rejects_missing_artifacts() {
    let request = request();
    let selected = selected_qa_image(RuntimeProfile::BrowserQa);
    let owner = uuid::Uuid::new_v4().hyphenated().to_string();
    let spec = QaDriverContainerSpec {
        owner_id: &owner,
        task_uid: &request.parent_task_uid,
        driver: "browser",
        request: &request,
        image: &selected,
        artifacts: &[],
        dependency_caches: &[],
        network: QaDriverNetwork::Disabled,
        service_lease: None,
        relay_binding: None,
        resources: &resources(),
    };
    let error = spec.create_body().unwrap_err();
    assert!(error.message.contains("artifacts"));
}

#[test]
fn desktop_driver_requires_the_exact_integrity_checked_dependency_cache_set() {
    use crate::execution::contract::DependencyCacheEcosystemV1;

    let mut request = request();
    request.relevant_platforms = vec![crate::runtime_qa::QaPlatform::LinuxDesktop];
    let planning_uid = uuid::Uuid::new_v4().hyphenated().to_string();
    request.planning_item_uid = Some(planning_uid.clone());
    let project_uid = uuid::Uuid::new_v4().hyphenated().to_string();
    let root = tempfile::tempdir().unwrap();
    std::fs::write(root.path().join("package.tgz"), b"approved offline package").unwrap();
    let cache = crate::harness::resource_bridge::CacheSnapshot::capture(
        root.path(),
        &planning_uid,
        DependencyCacheEcosystemV1::Npm,
        &project_uid,
        "sha256:synthetic-lockfile",
        "amd64",
        "local",
    )
    .unwrap();
    request.dependency_cache_refs = vec![crate::runtime_qa::QaDependencyCacheRefV1 {
        dependency_request_id: uuid::Uuid::new_v4().hyphenated().to_string(),
        cache_ref: cache.cache_ref.clone(),
    }];
    let image = selected_qa_image(RuntimeProfile::DesktopQa);
    let resources = resources();

    assert!(
        super::driver_cache::validate_for_driver(
            "desktop",
            &request,
            std::slice::from_ref(&cache),
            &image,
            &resources,
        )
        .is_ok()
    );
    assert!(
        super::driver_cache::validate_for_driver("desktop", &request, &[], &image, &resources,)
            .is_err()
    );

    let mut tampered_cache = cache.clone();
    tampered_cache.archive.push(0);
    assert!(
        super::driver_cache::validate_for_driver(
            "desktop",
            &request,
            &[tampered_cache],
            &image,
            &resources,
        )
        .is_err()
    );

    let mut wrong_arch = cache.clone();
    wrong_arch.cache_ref.target_arch = "arm64".into();
    assert!(
        super::driver_cache::validate_for_driver(
            "desktop",
            &request,
            &[wrong_arch],
            &image,
            &resources,
        )
        .is_err()
    );

    let mut tmpfs = HashMap::new();
    tmpfs.insert("/workspace".into(), "rw,nosuid,nodev,noexec".into());
    let mut env = Vec::new();
    super::driver_cache::configure_desktop_cache_mounts(
        "desktop",
        std::slice::from_ref(&cache),
        &image,
        &resources,
        &mut tmpfs,
        &mut env,
    )
    .unwrap();
    assert!(tmpfs["/workspace"].contains("uid=0,gid=0,mode=0755"));
    assert!(tmpfs.contains_key("/home/executor/.npm"));
    assert!(
        tmpfs["/workspace"]
            .split(',')
            .any(|option| option == "exec")
    );
    assert!(
        !tmpfs["/workspace"]
            .split(',')
            .any(|option| option == "noexec")
    );
    assert!(env.contains(&"NPM_CONFIG_OFFLINE=true".into()));
}

#[test]
fn rootless_qa_candidates_are_locked_but_runtime_stays_disabled_until_approved() {
    let lock = ImageLock::bundled().unwrap();
    for (name, profile) in [
        ("browser_qa", RuntimeProfile::BrowserQa),
        ("desktop_qa", RuntimeProfile::DesktopQa),
    ] {
        let variant = lock
            .profiles
            .get(name)
            .and_then(|variants| variants.first())
            .expect("rootless candidate is recorded in the image lock");
        assert_eq!(variant.platform, "linux/amd64");
        assert!(!variant.runtime_enabled);
        assert!(variant.build_recipe_sha256.is_some());
        assert_eq!(
            lock.select(profile, "amd64").unwrap_err().code,
            crate::execution::docker::discovery::PrerequisiteCode::ProfileUnsupported
        );
    }
}

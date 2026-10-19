use super::*;

fn request() -> crate::runtime_qa::QaRunRequestV1 {
    let uid = || uuid::Uuid::new_v4().hyphenated().to_string();
    crate::runtime_qa::QaRunRequestV1 {
        schema_version: 1,
        qa_run_id: uid(),
        parent_task_uid: uid(),
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

// Boundary: the app-owned Docker lifecycle must upload a bounded artifact,
// start the pinned QA driver's real smoke entrypoint, collect its output, and
// remove only that owned container on a rootless daemon. A unit test cannot
// prove Docker's upload/start/log/cleanup behavior. External dependency: a
// rootless Docker daemon with the just-built desktop candidate loaded. Setup:
// candidate image build plus one short Xvfb smoke run.
#[tokio::test]
#[ignore = "requires rootless Docker and a locally loaded desktop QA candidate"]
async fn rootless_app_owned_qa_driver_uploads_runs_reads_and_cleans_candidate() {
    use std::time::Duration;

    let image_reference = std::env::var("KOOLADE_QA_DESKTOP_IMAGE")
        .expect("candidate image reference is provided by the rootless QA workflow");
    let digest = std::env::var("KOOLADE_QA_DESKTOP_DIGEST")
        .expect("candidate image digest is provided by the rootless QA workflow");
    let docker = bollard::Docker::connect_with_local_defaults().unwrap();
    let info = docker.info().await.unwrap();
    assert!(
        info.security_options
            .unwrap_or_default()
            .iter()
            .any(|option| option.contains("name=rootless"))
    );

    let request = {
        let mut request = request();
        request.relevant_platforms = vec![crate::runtime_qa::QaPlatform::LinuxDesktop];
        request
    };
    let mut lock = ImageLock::bundled().unwrap();
    let variant = lock
        .profiles
        .get_mut("desktop_qa")
        .and_then(|variants| variants.iter_mut().find(|v| v.platform == "linux/amd64"))
        .unwrap();
    variant.reference = image_reference;
    variant.digest = digest;
    variant.runtime_enabled = true;
    let selected = lock.select(RuntimeProfile::DesktopQa, "amd64").unwrap();

    let root = tempfile::tempdir().unwrap();
    std::fs::write(
        root.path().join("snapshot.txt"),
        b"app-owned upload fixture",
    )
    .unwrap();
    let mount = QaArtifactMount::inspect(root.path(), "/workspace").unwrap();
    let (artifact_upload, _) = mount.snapshot_upload(1024, None).unwrap();
    let artifacts = [mount];
    let uploads = [artifact_upload];
    let resource_budget = resources();
    let spec = QaDriverContainerSpec {
        owner_id: &request.execution_session_id,
        request: &request,
        task_uid: &request.parent_task_uid,
        driver: "desktop",
        image: &selected,
        artifacts: &artifacts,
        network: QaDriverNetwork::Disabled,
        service_lease: None,
        relay_binding: None,
        resources: &resource_budget,
    };
    let mut body = spec.create_body_with_lock(&lock).unwrap();
    body.cmd = Some(vec!["--smoke".into()]);
    let name = format!("koolade-qa-{}", request.qa_run_id);
    let (_cancel, mut cancel) = tokio::sync::watch::channel(false);
    let output = run_ephemeral_driver(
        &docker,
        &name,
        body,
        &uploads,
        Duration::from_secs(90),
        1024 * 1024,
        &mut cancel,
    )
    .await
    .unwrap();
    let response: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(response["status"], "ready");
    assert_eq!(response["display_width"], 1440);
    assert!(response["screenshot_bytes"].as_u64().unwrap() > 0);
    match docker.inspect_container(&name, None).await {
        Err(bollard::errors::Error::DockerResponseServerError {
            status_code: 404, ..
        }) => {}
        other => panic!("owned QA container cleanup was not confirmed: {other:?}"),
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
        network: QaDriverNetwork::Disabled,
        service_lease: None,
        relay_binding: None,
        resources: &resources(),
    };
    let error = spec.create_body().unwrap_err();
    assert!(error.message.contains("artifacts"));
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

#[test]
fn desktop_driver_accepts_only_desktop_image_and_exact_platform() {
    let mut desktop_request = request();
    desktop_request.relevant_platforms = vec![crate::runtime_qa::QaPlatform::LinuxDesktop];
    let image = selected_qa_image(RuntimeProfile::DesktopQa);
    let owner = uuid::Uuid::new_v4().hyphenated().to_string();
    let root = std::env::temp_dir().join(format!("qa-desktop-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&root).unwrap();
    let mount = QaArtifactMount::inspect(&root, "/workspace").unwrap();
    let resource_budget = resources();
    let spec = QaDriverContainerSpec {
        owner_id: &owner,
        request: &desktop_request,
        task_uid: &desktop_request.parent_task_uid,
        driver: "desktop",
        image: &image,
        artifacts: &[mount],
        network: QaDriverNetwork::Disabled,
        service_lease: None,
        relay_binding: None,
        resources: &resource_budget,
    };
    assert!(spec.validate().is_ok());
    let _ = std::fs::remove_dir_all(root);

    let mut mismatched_request = desktop_request.clone();
    mismatched_request.relevant_platforms = vec![crate::runtime_qa::QaPlatform::Web];
    let mismatched = QaDriverContainerSpec {
        request: &mismatched_request,
        ..spec
    };
    assert!(mismatched.validate().is_err());

    let mut desktop_request = request();
    desktop_request.relevant_platforms = vec![crate::runtime_qa::QaPlatform::LinuxDesktop];
    let browser_image = selected_qa_image(RuntimeProfile::BrowserQa);
    let root = std::env::temp_dir().join(format!("qa-desktop-mismatch-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&root).unwrap();
    let mount = QaArtifactMount::inspect(&root, "/workspace").unwrap();
    let owner = uuid::Uuid::new_v4().hyphenated().to_string();
    let resource_budget = resources();
    let wrong_image = QaDriverContainerSpec {
        owner_id: &owner,
        task_uid: &desktop_request.parent_task_uid,
        driver: "desktop",
        request: &desktop_request,
        image: &browser_image,
        artifacts: &[mount],
        network: QaDriverNetwork::Disabled,
        service_lease: None,
        relay_binding: None,
        resources: &resource_budget,
    };
    assert!(wrong_image.validate().is_err());
    let _ = std::fs::remove_dir_all(root);
}

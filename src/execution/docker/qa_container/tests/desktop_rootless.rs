use super::*;

#[cfg(target_os = "linux")]
#[path = "desktop_rootless/packaged_linux.rs"]
mod packaged_linux;
#[cfg(target_os = "linux")]
#[path = "desktop_rootless/task_artifact.rs"]
mod task_artifact;
#[cfg(target_os = "linux")]
pub(super) use task_artifact::exercise_prepared_dispatch;

pub(super) fn candidate_desktop_lock() -> ImageLock {
    let image_reference = std::env::var("KOOLADE_QA_DESKTOP_IMAGE")
        .expect("desktop QA candidate reference comes from rootless CI");
    let digest = std::env::var("KOOLADE_QA_DESKTOP_DIGEST")
        .expect("desktop QA candidate digest comes from rootless CI");
    let mut lock = ImageLock::bundled().unwrap();
    let variant = lock
        .profiles
        .get_mut("desktop_qa")
        .and_then(|variants| variants.iter_mut().find(|v| v.platform == "linux/amd64"))
        .unwrap();
    variant.reference = image_reference;
    variant.digest = digest;
    variant.runtime_enabled = true;
    lock
}

// Boundary: the app-owned Docker lifecycle must stage a bounded artifact and
// request-bound package cache into rootless tmpfs, verify the cache is readable
// with offline-only settings, run the pinned desktop interaction fixture, and
// remove only that owned container.
// A unit test cannot
// prove Docker's upload/start/log/cleanup behavior. External dependency: a
// Docker daemon with the just-built desktop candidate loaded (rootless in CI).
// Setup: candidate image build, one small synthetic cache snapshot, and one
// short Xvfb smoke run.
#[tokio::test]
#[ignore = "requires rootless Docker and a locally loaded desktop QA candidate"]
async fn rootless_app_owned_qa_driver_uploads_runs_reads_and_cleans_candidate() {
    use std::time::Duration;

    let docker = bollard::Docker::connect_with_local_defaults().unwrap();
    let info = docker.info().await.unwrap();
    if std::env::var_os("KOOLADE_QA_REQUIRE_ROOTLESS").is_some() {
        assert!(
            info.security_options
                .unwrap_or_default()
                .iter()
                .any(|option| option.contains("name=rootless"))
        );
    }

    let cache_root = tempfile::tempdir().unwrap();
    std::fs::write(
        cache_root.path().join("package.tgz"),
        b"approved desktop cache fixture",
    )
    .unwrap();
    let planning_uid = uuid::Uuid::new_v4().hyphenated().to_string();
    let project_uid = uuid::Uuid::new_v4().hyphenated().to_string();
    let cache = crate::harness::resource_bridge::CacheSnapshot::capture(
        cache_root.path(),
        &planning_uid,
        crate::execution::contract::DependencyCacheEcosystemV1::Npm,
        &project_uid,
        "sha256:synthetic-lockfile",
        "amd64",
        "local",
    )
    .unwrap();
    let mut request = request();
    request.relevant_platforms = vec![crate::runtime_qa::QaPlatform::LinuxDesktop];
    request.planning_item_uid = Some(planning_uid);
    request.dependency_cache_refs = vec![crate::runtime_qa::QaDependencyCacheRefV1 {
        dependency_request_id: uuid::Uuid::new_v4().hyphenated().to_string(),
        cache_ref: cache.cache_ref.clone(),
    }];
    let lock = candidate_desktop_lock();
    let selected = lock.select(RuntimeProfile::DesktopQa, "amd64").unwrap();

    let root = tempfile::tempdir().unwrap();
    std::fs::create_dir(root.path().join("dist")).unwrap();
    std::fs::write(
        root.path().join("dist/snapshot.txt"),
        b"app-owned upload fixture",
    )
    .unwrap();
    let mount = QaArtifactMount::inspect(root.path(), "/workspace").unwrap();
    let (artifact_upload, _) = mount.snapshot_upload(1024, None).unwrap();
    let artifacts = [mount];
    let cache_upload = QaContainerUpload {
        target: "/home/executor/.npm".into(),
        archive: crate::execution::docker::qa_container::archive_for_runtime_user(
            &cache,
            &selected.runtime_user,
        )
        .unwrap(),
    };
    let uploads = [artifact_upload, cache_upload];
    let mut resource_budget = resources();
    resource_budget.process_count = 128;
    let spec = QaDriverContainerSpec {
        owner_id: &request.execution_session_id,
        request: &request,
        task_uid: &request.parent_task_uid,
        driver: "desktop",
        image: &selected,
        artifacts: &artifacts,
        dependency_caches: std::slice::from_ref(&cache),
        network: QaDriverNetwork::Disabled,
        service_lease: None,
        relay_binding: None,
        resources: &resource_budget,
    };
    let mut body = spec.create_body_with_lock(&lock).unwrap();
    let host_config = body.host_config.as_ref().unwrap();
    assert_eq!(host_config.readonly_rootfs, Some(true));
    let tmpfs = host_config.tmpfs.as_ref().unwrap();
    for path in ["/tmp", "/run", "/home/executor", "/workspace"] {
        let options = tmpfs
            .get(path)
            .unwrap_or_else(|| panic!("missing tmpfs mount {path}"));
        assert!(options.starts_with("rw,"), "{path} must be writable tmpfs");
        assert!(options.contains("uid=10001,gid=10001"));
    }
    assert!(tmpfs.contains_key("/home/executor/.npm"));
    let tmpfs_budget = tmpfs
        .values()
        .map(|options| {
            options
                .split("size=")
                .nth(1)
                .and_then(|size| size.split(',').next())
                .and_then(|size| size.parse::<u64>().ok())
                .expect("tmpfs mount has a byte limit")
        })
        .sum::<u64>();
    assert!(tmpfs_budget <= resource_budget.temporary_disk_bytes);
    assert!(
        body.env
            .as_ref()
            .unwrap()
            .contains(&"NPM_CONFIG_OFFLINE=true".into())
    );
    body.cmd = Some(vec!["--fixture-cache-smoke".into()]);
    let name = format!("koolade-qa-{}", request.qa_run_id);
    let (_cancel, mut cancel) = tokio::sync::watch::channel(false);
    let container_id = create_driver_container(
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
    let output = run_created_driver_with_status(
        &docker,
        &container_id,
        Duration::from_secs(90),
        1024 * 1024,
        &mut cancel,
    )
    .await;
    let cleanup = cleanup_driver_container(&docker, &container_id).await;
    let output = match output {
        Ok(output) => output,
        Err(error) => panic!("QA driver execution failed: {error:?}; cleanup={cleanup:?}"),
    };
    cleanup.unwrap();
    assert_eq!(
        output.exit_code,
        0,
        "stdout={:?}, stderr={:?}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let response: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(
        response["status"],
        "passed",
        "stdout={:?}, stderr={:?}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(response["cache_readable"], true);
    assert_eq!(response["offline"], true);
    assert_eq!(response["interaction_case"]["click_assertion"], "passed");
    assert_eq!(response["interaction_case"]["keyboard_assertion"], "passed");
    if std::env::var_os("KOOLADE_QA_REQUIRE_EGUI").is_some() {
        assert_eq!(response["egui_application"]["status"], "passed");
        assert_eq!(
            response["egui_application"]["screenshots"]
                .as_array()
                .unwrap()
                .len(),
            3
        );
    }
    match docker.inspect_container(&name, None).await {
        Err(bollard::errors::Error::DockerResponseServerError {
            status_code: 404, ..
        }) => {}
        other => panic!("owned QA container cleanup was not confirmed: {other:?}"),
    }
    #[cfg(target_os = "linux")]
    task_artifact::exercise(&docker, &selected, &lock, &resource_budget).await;
}

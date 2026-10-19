use super::*;

// Boundary: the app-owned Docker lifecycle must stage a bounded artifact into
// tmpfs, start the pinned QA driver's real desktop fixture, collect its
// interaction and screenshot report, and remove only that owned container.
// A unit test cannot
// prove Docker's upload/start/log/cleanup behavior. External dependency: a
// Docker daemon with the just-built desktop candidate loaded (rootless in CI).
// Setup:
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
    if std::env::var_os("KOOLADE_QA_REQUIRE_ROOTLESS").is_some() {
        assert!(
            info.security_options
                .unwrap_or_default()
                .iter()
                .any(|option| option.contains("name=rootless"))
        );
    }

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
    std::fs::create_dir(root.path().join("dist")).unwrap();
    std::fs::write(
        root.path().join("dist/snapshot.txt"),
        b"app-owned upload fixture",
    )
    .unwrap();
    let mount = QaArtifactMount::inspect(root.path(), "/workspace").unwrap();
    let (artifact_upload, _) = mount.snapshot_upload(1024, None).unwrap();
    let artifacts = [mount];
    let uploads = [artifact_upload];
    let mut resource_budget = resources();
    resource_budget.process_count = 128;
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
    body.cmd = Some(vec!["--fixture-smoke".into()]);
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
    assert_eq!(
        response["status"],
        "passed",
        "stdout={:?}, stderr={:?}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
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
}

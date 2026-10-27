use super::*;

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
        dependency_caches: &[],
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
        dependency_caches: &[],
        network: QaDriverNetwork::Disabled,
        service_lease: None,
        relay_binding: None,
        resources: &resource_budget,
    };
    assert!(wrong_image.validate().is_err());
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn only_desktop_driver_workspace_allows_executing_staged_artifacts() {
    let mut request = request();
    request.relevant_platforms = vec![crate::runtime_qa::QaPlatform::LinuxDesktop];
    let mut lock = ImageLock::bundled().unwrap();
    for profile in ["browser_qa", "desktop_qa"] {
        for variant in lock.profiles.get_mut(profile).unwrap() {
            variant.runtime_enabled = true;
        }
    }
    let desktop_image = lock.select(RuntimeProfile::DesktopQa, "amd64").unwrap();
    let root = tempfile::tempdir().unwrap();
    let artifact = QaArtifactMount::inspect(root.path(), "/workspace").unwrap();
    let resources = resources();
    let desktop = QaDriverContainerSpec {
        owner_id: &request.execution_session_id,
        request: &request,
        task_uid: &request.parent_task_uid,
        driver: "desktop",
        image: &desktop_image,
        artifacts: std::slice::from_ref(&artifact),
        dependency_caches: &[],
        network: QaDriverNetwork::Disabled,
        service_lease: None,
        relay_binding: None,
        resources: &resources,
    };
    let workspace_options = desktop
        .create_body_with_lock(&lock)
        .unwrap()
        .host_config
        .unwrap()
        .tmpfs
        .unwrap()
        .remove("/workspace")
        .unwrap();
    assert!(workspace_options.split(',').any(|option| option == "exec"));
    assert!(
        !workspace_options
            .split(',')
            .any(|option| option == "noexec")
    );
    assert_eq!(
        super::super::workspace_execution_option("browser"),
        "noexec"
    );
    assert_eq!(
        super::super::workspace_execution_option("service"),
        "noexec"
    );
}

#[test]
fn service_driver_requires_headless_platform_and_exact_preview_lease() {
    let mut request = request();
    request.relevant_platforms = vec![crate::runtime_qa::QaPlatform::HeadlessService];
    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64;
    let task_container_id = "1".repeat(64);
    let task_network_id = "2".repeat(64);
    let relay_container_id = "3".repeat(64);
    let relay_network_id = "4".repeat(64);
    let lease = crate::runtime_qa::QaTaskServiceLeaseV1 {
        schema_version: 1,
        qa_run_id: request.qa_run_id.clone(),
        task_uid: request.parent_task_uid.clone(),
        task_container_id: task_container_id.clone(),
        task_network_id: task_network_id.clone(),
        preview_port: 8765,
        task_build_sha: request.build_artifact_sha256.clone(),
        approved_spec_hash: request.approved_spec_hash.clone(),
        startup_recipe_ref: request.startup_recipe_ref.clone(),
        startup_recipe_sha256: request.startup_recipe_sha256.clone(),
        internal_network_only: true,
        published_ports: false,
        expires_at_unix_ms: now_ms + 60_000,
    };
    let binding = crate::runtime_qa::QaPreviewRelayBindingV1 {
        schema_version: 1,
        qa_run_id: request.qa_run_id.clone(),
        task_uid: request.parent_task_uid.clone(),
        task_execution_id: request.execution_session_id.clone(),
        task_container_id,
        relay_container_id: relay_container_id.clone(),
        relay_token: "0123456789abcdef".repeat(4),
        browser_container_id: None,
        task_network_id,
        relay_browser_network_id: relay_network_id.clone(),
        target_preview_port: 8765,
        relay_port: 8080,
        preview_origin: "http://qa-preview:8080".into(),
        task_build_sha: request.build_artifact_sha256.clone(),
        approved_spec_hash: request.approved_spec_hash.clone(),
        startup_recipe_id: request.startup_recipe_ref.clone(),
        startup_recipe_sha256: request.startup_recipe_sha256.clone(),
        internal_networks_only: true,
        published_ports: false,
        recorded_at_unix_ms: now_ms,
    };
    let image = selected_qa_image(RuntimeProfile::BrowserQa);
    let root = tempfile::tempdir().unwrap();
    let artifact = QaArtifactMount::inspect(root.path(), "/workspace").unwrap();
    let budget = resources();
    let spec = QaDriverContainerSpec {
        owner_id: &request.execution_session_id,
        task_uid: &request.parent_task_uid,
        driver: "service",
        request: &request,
        image: &image,
        artifacts: &[artifact],
        dependency_caches: &[],
        network: QaDriverNetwork::InternalPreviewRelay {
            network_id: relay_network_id,
            relay_container_id,
        },
        service_lease: Some(&lease),
        relay_binding: Some(&binding),
        resources: &budget,
    };
    assert!(spec.validate().is_ok());

    let mismatched_binding = crate::runtime_qa::QaPreviewRelayBindingV1 {
        task_execution_id: uuid::Uuid::new_v4().hyphenated().to_string(),
        ..binding.clone()
    };
    let mismatched = QaDriverContainerSpec {
        relay_binding: Some(&mismatched_binding),
        ..spec.clone()
    };
    assert!(mismatched.validate().is_err());

    let mismatched_lease = crate::runtime_qa::QaTaskServiceLeaseV1 {
        preview_port: 8766,
        ..lease.clone()
    };
    let mismatched = QaDriverContainerSpec {
        service_lease: Some(&mismatched_lease),
        ..spec
    };
    assert!(mismatched.validate().is_err());
}

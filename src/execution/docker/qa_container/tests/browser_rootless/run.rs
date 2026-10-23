use super::*;

/// Boundary: app-owned startup uploads a bounded Python web artifact,
/// provisions its internal preview relay, runs the pinned browser driver,
/// parses screenshot and interaction evidence, rejects browser requests to
/// metadata, host-local and off-origin destinations, and cleans only owned
/// resources. Unit tests cannot prove rootless Docker networking, preview
/// reachability, browser execution, request containment, or container cleanup.
/// External dependency: rootless Docker plus the just-built browser candidate
/// image. Setup cost: one image build and several short browser scenarios.
#[tokio::test]
#[ignore = "requires rootless Docker and a locally loaded browser QA candidate"]
async fn rootless_app_owned_browser_preview_and_driver_produce_evidence_and_clean_up() {
    let image_reference = std::env::var("KOOLADE_QA_BROWSER_IMAGE")
        .expect("candidate image reference is provided by the rootless QA workflow");
    let digest = std::env::var("KOOLADE_QA_BROWSER_DIGEST")
        .expect("candidate image digest is provided by the rootless QA workflow");
    let docker = bollard::Docker::connect_with_local_defaults().unwrap();
    let info = docker.info().await.unwrap();
    assert!(
        info.security_options
            .unwrap_or_default()
            .iter()
            .any(|option| option.contains("name=rootless")),
        "browser application lifecycle must run on rootless Docker"
    );

    let mut lock = ImageLock::bundled().unwrap();
    let variant = lock
        .profiles
        .get_mut("browser_qa")
        .and_then(|variants| {
            variants
                .iter_mut()
                .find(|variant| variant.platform == "linux/amd64")
        })
        .expect("browser QA candidate is present in the reviewed image lock");
    variant.reference = image_reference;
    variant.digest = digest;
    variant.runtime_enabled = true;
    let browser_image = lock.select(RuntimeProfile::BrowserQa, "amd64").unwrap();
    let service_image = lock.select(RuntimeProfile::Python, "amd64").unwrap();

    let request = {
        let mut request = request();
        request.bounded_limits = QaBoundedLimitsV1 {
            max_scenarios: 1,
            max_actions_per_scenario: 8,
            max_total_actions: 8,
            timeout_seconds: 90,
            max_evidence_bytes: 16 * 1024 * 1024,
            max_screenshots: 4,
        };
        request.driver_profile_version = browser_image.tool_versions["driver"].clone();
        request
    };
    let recipe = ExecutionRecipeV1 {
        schema_version: 1,
        recipe_id: "python-preview".into(),
        source: RecipeSource::UserApproved,
        working_directory_relative: ".".into(),
        executable_id: "python".into(),
        argv: vec!["/workspace/server.py".into()],
        allowed_env_keys: vec!["HOST".into(), "PORT".into()],
        allowed_output_paths: vec!["dist".into()],
        timeout_seconds: 90,
        expected_exit_codes: vec![0],
        expected_artifacts: vec![],
        cache_scope: None,
        prerequisite_tool_ids: vec!["python".into()],
        network_requirement: NetworkRequirement::ApprovedServicePorts,
        evidence_refs: vec!["user-approval:rootless-python-preview-fixture".into()],
    };
    let recipe_hash = format!(
        "{:x}",
        sha2::Sha256::digest(serde_json::to_vec(&recipe).unwrap())
    );
    let mut request = request;
    request.startup_recipe_ref = recipe.recipe_id.clone();
    request.startup_recipe_sha256 = recipe_hash;

    let fixture = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/runtime_qa/browser/server.py");
    let artifact_root = tempfile::tempdir().unwrap();
    std::fs::copy(fixture, artifact_root.path().join("server.py")).unwrap();
    let artifact_hash = format!(
        "{:x}",
        sha2::Sha256::digest(std::fs::read(artifact_root.path().join("server.py")).unwrap())
    );
    request.build_artifact_sha256 = artifact_hash.clone();
    let (artifact_upload, _) = snapshot_selected_files(
        artifact_root.path(),
        &["server.py".into()],
        &BTreeMap::from([("server.py".into(), artifact_hash)]),
        1024 * 1024,
    )
    .unwrap();

    let scenario = QaScenarioV1 {
        schema_version: 1,
        id: uuid::Uuid::new_v4().hyphenated().to_string(),
        requirement_ids: request.requirement_ids.clone(),
        preconditions: vec!["The approved Python preview is running.".into()],
        steps: vec![
            step(
                QaDriverActionV1::BrowserOpen {
                    url: "http://qa-service/".into(),
                },
                "Preview fixture is visible",
                true,
            ),
            step(
                QaDriverActionV1::BrowserFill {
                    locator: QaBrowserLocatorV1::Label {
                        label: "Message".into(),
                    },
                    value: "rootless synthetic input".into(),
                },
                "Message field contains synthetic input",
                false,
            ),
            step(
                QaDriverActionV1::BrowserPress {
                    locator: QaBrowserLocatorV1::Label {
                        label: "Message".into(),
                    },
                    key: "Enter".into(),
                },
                "Form submission completes",
                false,
            ),
            step(
                QaDriverActionV1::BrowserAssertText {
                    locator: QaBrowserLocatorV1::TestId {
                        test_id: "status".into(),
                    },
                    expected_text: "Saved".into(),
                },
                "Saved state is observed",
                true,
            ),
        ],
        expected_results: vec!["Submitting the form updates the page to Saved.".into()],
        negative_paths: vec![],
        // This test double reviews functional traces and metadata, not pixels.
        // Pixel expectations are covered by the fail-closed visual-review test.
        visual_expectations: vec![],
        redaction_regions: vec![],
        priority: 1,
    };
    request.validate().unwrap();
    scenario.validate(&request).unwrap();
    let browser_request = build_browser_request(&request, &scenario).unwrap();
    let request_upload = QaContainerUpload::request_file(&browser_request, 256 * 1024).unwrap();
    let (_cancel_sender, mut cancel) = tokio::sync::watch::channel(false);
    let mut resources = resources();
    resources.process_count = 128;
    let preview_spec = QaWebPreviewSpec {
        request: &request,
        browser_image: &browser_image,
        service_image: &service_image,
        recipe: &recipe,
        artifact_upload: &artifact_upload,
        dependency_caches: &[],
        preview_port: 4173,
        health_path: "/",
        resources: &resources,
    };
    let preview = super::super::service::start_web_preview_with_lock(
        &docker,
        preview_spec,
        &mut cancel,
        &lock,
    )
    .await
    .unwrap();
    validate_preview_relay_network(
        &docker,
        &request,
        &preview.service_lease,
        &preview.relay_binding,
    )
    .await
    .unwrap();

    let mut driver_container_id = None;
    let driver_result: Result<_, String> = async {
        let artifacts = [QaArtifactMount::inspect(artifact_root.path(), "/workspace")
            .map_err(|error| error.message)?];
        let uploads = [artifact_upload.clone(), request_upload];
        let spec = QaDriverContainerSpec {
            owner_id: &request.execution_session_id,
            request: &request,
            task_uid: &request.parent_task_uid,
            driver: "browser",
            image: &browser_image,
            artifacts: &artifacts,
            dependency_caches: &[],
            network: QaDriverNetwork::InternalPreviewRelay {
                network_id: preview.relay_binding.relay_browser_network_id.clone(),
                relay_container_id: preview.relay_binding.relay_container_id.clone(),
            },
            service_lease: Some(&preview.service_lease),
            relay_binding: Some(&preview.relay_binding),
            resources: &resources,
        };
        let body = spec
            .create_body_with_lock(&lock)
            .map_err(|error| error.message)?;
        let name = format!("koolade-qa-{}", request.qa_run_id);
        let driver_id = create_driver_container(
            &docker,
            &name,
            body,
            &uploads,
            Duration::from_secs(120),
            16 * 1024 * 1024,
            &mut cancel,
        )
        .await
        .map_err(|error| error.message)?;
        driver_container_id = Some(driver_id.clone());
        let mut relay_binding = preview.relay_binding.clone();
        relay_binding.browser_container_id = Some(driver_id.clone());
        validate_browser_driver_network(&docker, &request, &relay_binding, &driver_id)
            .await
            .map_err(|error| error.message)?;
        let output = run_created_driver(
            &docker,
            &driver_id,
            Duration::from_secs(120),
            16 * 1024 * 1024,
            &mut cancel,
        )
        .await
        .map_err(|error| error.message)?;
        parse_browser_output(&output.stdout, &request, &scenario)
    }
    .await;
    if let Some(driver_id) = driver_container_id.take() {
        cleanup_driver_container(&docker, &driver_id).await.unwrap();
    }
    let origin_escape_result = redirect::assert_origin_escape_cases(
        &docker,
        &request,
        &browser_image,
        &lock,
        &artifact_root,
        &artifact_upload,
        &preview,
        &resources,
        &mut cancel,
    )
    .await;
    let seeded_defect_result = defects::assert_seeded_wrong_destination_is_reported(
        &docker,
        &request,
        &browser_image,
        &lock,
        &artifact_root,
        &artifact_upload,
        &preview,
        &resources,
        &mut cancel,
    )
    .await;
    cleanup::cleanup_and_assert(&docker, &request, driver_container_id, &preview).await;
    origin_escape_result.unwrap_or_else(|error| panic!("origin escape was not blocked: {error}"));
    seeded_defect_result
        .unwrap_or_else(|error| panic!("seeded browser defect was not reported: {error}"));
    let output = driver_result.unwrap();
    assert!(output.completed);
    assert_eq!(output.assertions.len(), 1);
    assert_eq!(
        output.assertions[0].status,
        crate::runtime_qa::QaAssertionStatus::Passed
    );
    assert!(output.screenshots.iter().any(|screen| screen.width >= 320));
    lifecycle::review_and_persist(
        &request,
        scenario,
        &preview.service_lease,
        output,
        artifact_root.path().join("qa-evidence"),
    );
}

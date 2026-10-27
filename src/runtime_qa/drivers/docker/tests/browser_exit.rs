use super::*;
use crate::runtime_qa::QaPlatform;

#[test]
// Production boundary: the app-owned browser adapter receives the process
// exit code together with the driver's structured failure report. This checks
// the session-to-driver path with an in-process backend; the hosted rootless
// Docker job covers engine collection and container isolation.
fn browser_driver_preserves_structured_failure_from_nonzero_exit() {
    let mut request = request();
    request.relevant_platforms = vec![QaPlatform::Web];
    request.driver_profile_version = "browser-driver/v1".into();
    let mut scenario = scenario();
    scenario.steps = vec![crate::runtime_qa::QaScenarioStepV1 {
        driver_action: crate::runtime_qa::QaDriverActionV1::BrowserOpen {
            url: "http://qa-service/".into(),
        },
        expected_state: None,
        screenshot_checkpoint: false,
    }];
    let report = serde_json::json!({
        "schema_version": 1,
        "qa_run_id": request.qa_run_id,
        "scenario_id": scenario.id,
        "task_uid": request.parent_task_uid,
        "code_sha": request.code_head_sha,
        "build_sha256": request.build_artifact_sha256,
        "approved_spec_hash": request.approved_spec_hash,
        "driver_profile_version": request.driver_profile_version,
        "outcome": "failed",
        "origin": "http://qa-preview:8080",
        "viewport": {"width": 1280, "height": 720},
        "driver_version": "playwright-pinned",
        "actions": [],
        "screenshots": [],
        "console_errors": [],
        "failed_requests": [],
        "policy_violation": null,
        "error": "browser interaction failed"
    });
    let backend_output = DriverContainerOutput {
        exit_code: 1,
        stdout: serde_json::to_vec(&report).unwrap(),
        stderr: Vec::new(),
    };
    let (output, observations) = run_web_driver(&request, &scenario, backend_output);
    let output = output.unwrap();
    assert!(!output.completed);
    assert_eq!(
        output.failure.as_deref(),
        Some("browser interaction failed")
    );
    assert_eq!(
        observations.lock().unwrap().phases,
        ["upload", "read", "cleanup"]
    );
}

#[test]
// Production boundary: a browser process that exits before emitting JSON must
// preserve bounded stderr in the adapter error so orchestration can save a
// sanitized InfrastructureBlocked report. In-process adapter only; hosted
// rootless Docker remains the engine and isolation proof.
fn browser_startup_error_keeps_bounded_stderr_for_orchestration() {
    let mut request = request();
    request.relevant_platforms = vec![QaPlatform::Web];
    request.driver_profile_version = "browser-driver/v1".into();
    let mut scenario = scenario();
    scenario.steps = vec![crate::runtime_qa::QaScenarioStepV1 {
        driver_action: crate::runtime_qa::QaDriverActionV1::BrowserOpen {
            url: "http://qa-service/".into(),
        },
        expected_state: None,
        screenshot_checkpoint: false,
    }];
    let backend_output = DriverContainerOutput {
        exit_code: 2,
        stdout: Vec::new(),
        stderr: b"browser launch failed at /private/runtime token=secret".to_vec(),
    };
    let (result, observations) = run_web_driver(&request, &scenario, backend_output);

    let error = result.unwrap_err();
    assert!(error.contains("browser driver returned malformed JSON"));
    assert!(error.contains("browser launch failed"));
    assert_eq!(
        observations.lock().unwrap().phases,
        ["upload", "read", "cleanup"]
    );
}

fn run_web_driver(
    request: &crate::runtime_qa::QaRunRequestV1,
    scenario: &crate::runtime_qa::QaScenarioV1,
    backend_output: DriverContainerOutput,
) -> (
    Result<crate::runtime_qa::QaDriverOutputV1, String>,
    Arc<Mutex<Observations>>,
) {
    let task_id = request.parent_task_uid.clone();
    let image_digest = format!("sha256:{}", "a".repeat(64));
    let image = SelectedImage {
        profile: RuntimeProfile::BrowserQa,
        platform: "linux/amd64".into(),
        reference: format!("ghcr.io/zbarno/koolade-runtime-qa-browser@{image_digest}"),
        digest: image_digest.clone(),
        runtime_user: "10001:10001".into(),
        tool_versions: Default::default(),
    };
    let observations = Arc::new(Mutex::new(Observations::default()));
    let session = ExecutionSession::new(
        Arc::new(FakeBackend(Arc::clone(&observations), backend_output)),
        WorkspaceRef {
            project_id: uuid(),
            task_id,
            source_sha: request.code_head_sha.clone(),
        },
        ExecutionTarget::local(),
        ExecutionModeV1::RuntimeQa,
        image_digest,
        ResourceBudgetV1::default(),
    )
    .unwrap();
    let task_service_lease = crate::runtime_qa::QaTaskServiceLeaseV1 {
        schema_version: 1,
        qa_run_id: request.qa_run_id.clone(),
        task_uid: request.parent_task_uid.clone(),
        task_container_id: "b".repeat(64),
        task_network_id: "c".repeat(64),
        preview_port: 4173,
        task_build_sha: request.build_artifact_sha256.clone(),
        approved_spec_hash: request.approved_spec_hash.clone(),
        startup_recipe_ref: request.startup_recipe_ref.clone(),
        startup_recipe_sha256: request.startup_recipe_sha256.clone(),
        internal_network_only: true,
        published_ports: false,
        expires_at_unix_ms: u64::MAX,
    };
    let relay_binding = crate::runtime_qa::QaPreviewRelayBindingV1 {
        schema_version: 1,
        qa_run_id: request.qa_run_id.clone(),
        task_uid: request.parent_task_uid.clone(),
        task_execution_id: request.execution_session_id.clone(),
        task_container_id: "b".repeat(64),
        relay_container_id: "c".repeat(64),
        relay_token: "0123456789abcdef".repeat(4),
        browser_container_id: None,
        task_network_id: "e".repeat(64),
        relay_browser_network_id: "f".repeat(64),
        target_preview_port: 4173,
        relay_port: 8080,
        preview_origin: "http://qa-preview:8080".into(),
        task_build_sha: request.build_artifact_sha256.clone(),
        approved_spec_hash: request.approved_spec_hash.clone(),
        startup_recipe_id: request.startup_recipe_ref.clone(),
        startup_recipe_sha256: request.startup_recipe_sha256.clone(),
        internal_networks_only: true,
        published_ports: false,
        recorded_at_unix_ms: 1,
    };
    let temp = tempfile::tempdir().unwrap();
    std::fs::write(temp.path().join("app"), b"fixture application").unwrap();
    let artifact_mount = QaArtifactMount::inspect(temp.path(), "/workspace").unwrap();
    let (artifact_upload, _) = artifact_mount.snapshot_upload(1024, None).unwrap();
    let store = RuntimeQaStore::new(temp.path().join("qa-evidence"));
    let mut cancellation = tokio::sync::watch::channel(false).1;
    let mut driver = super::super::DockerQaScenarioDriver {
        session: &session,
        request,
        image: &image,
        artifact_mount,
        artifact_upload: &artifact_upload,
        dependency_caches: &[],
        resources: &ResourceBudgetV1::default(),
        desktop_target: None,
        service_lease: Some(&task_service_lease),
        relay_binding: Some(&relay_binding),
        store: &store,
        cancellation: &mut cancellation,
    };

    let output = driver.run_scenario(
        request,
        scenario,
        &QaTargetAuthorization::Web(&task_service_lease),
    );
    (output, observations)
}

#[test]
fn browser_nonzero_exit_keeps_structured_failure_and_cannot_be_mistaken_for_a_pass() {
    let scenario = scenario();
    let assertion = |status| crate::runtime_qa::QaAssertionV1 {
        id: uuid(),
        scenario_ids: vec![scenario.id.clone()],
        requirement_ids: vec!["REQ-1".into()],
        expected: "the save completes".into(),
        observed: "the browser action failed".into(),
        status,
        evidence_refs: Vec::new(),
    };
    let mut interaction_failure = driver_output();
    interaction_failure.assertions = vec![assertion(crate::runtime_qa::QaAssertionStatus::Failed)];
    super::super::apply_browser_exit_status(1, &mut interaction_failure);
    assert!(interaction_failure.completed);
    assert!(interaction_failure.failure.is_none());

    let mut inconsistent_success = driver_output();
    inconsistent_success.assertions = vec![assertion(crate::runtime_qa::QaAssertionStatus::Passed)];
    super::super::apply_browser_exit_status(1, &mut inconsistent_success);
    assert!(!inconsistent_success.completed);
    assert!(inconsistent_success.failure.is_some());
}

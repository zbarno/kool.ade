use std::{os::unix::fs::PermissionsExt, path::Path, time::Duration};

use sha2::Digest;

use crate::{
    execution::{
        contract::ResourceBudgetV1,
        docker::{
            images::{ImageLock, SelectedImage},
            qa_container::{QaArtifactMount, QaContainerUpload, QaDriverContainerSpec},
        },
    },
    project_runtime::{ExecutionRecipeV1, NetworkRequirement, RecipeSource},
    runtime_qa::{QaDriverActionV1, QaScenarioStepV1, QaScenarioV1, QaValidatedTargetV1},
};

// Production boundary: a separately built, approved executable is snapshotted
// into the task workspace, launched by the app-owned rootless desktop QA
// driver, and exercised through X11 with evidence returned to the app. Unit
// tests cannot prove the artifact upload, target process ownership, Xvfb
// interaction, response capture, and container cleanup work together.
// External dependency: rootless Docker with the candidate image. Setup cost:
// one fixture binary build and one short desktop run after the image build.
pub(super) async fn exercise(
    docker: &bollard::Docker,
    image: &SelectedImage,
    lock: &ImageLock,
    resources: &ResourceBudgetV1,
) {
    let fixture_path = std::env::var("KOOLADE_QA_DESKTOP_FIXTURE_EXECUTABLE")
        .expect("CI provides the separately built desktop fixture executable");
    let fixture_path = Path::new(&fixture_path);
    let metadata = std::fs::symlink_metadata(fixture_path).unwrap();
    assert!(metadata.is_file() && !metadata.file_type().is_symlink());
    assert!(metadata.permissions().mode() & 0o111 != 0);

    let task = tempfile::tempdir().unwrap();
    let executable = task.path().join("approved-desktop-app");
    std::fs::copy(fixture_path, &executable).unwrap();
    std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o555)).unwrap();
    let mount = QaArtifactMount::inspect(task.path(), "/workspace").unwrap();
    let (artifact_upload, artifact_sha256) = mount
        .snapshot_upload(64 * 1024 * 1024, None)
        .expect("fixture app artifact snapshot");

    let startup_recipe = ExecutionRecipeV1 {
        schema_version: 1,
        recipe_id: "approved-desktop-fixture-start".into(),
        source: RecipeSource::UserApproved,
        working_directory_relative: ".".into(),
        executable_id: "approved-desktop-app".into(),
        argv: Vec::new(),
        allowed_env_keys: Vec::new(),
        allowed_output_paths: Vec::new(),
        timeout_seconds: 45,
        expected_exit_codes: vec![0],
        expected_artifacts: Vec::new(),
        cache_scope: None,
        prerequisite_tool_ids: Vec::new(),
        network_requirement: NetworkRequirement::Denied,
        evidence_refs: vec!["user-approval:rootless-desktop-fixture".into()],
    };
    startup_recipe.validate().unwrap();
    let mut request = super::super::request();
    request.relevant_platforms = vec![crate::runtime_qa::QaPlatform::LinuxDesktop];
    request.build_artifact_sha256 = artifact_sha256;
    request.startup_recipe_ref = startup_recipe.recipe_id.clone();
    request.startup_recipe_sha256 = format!(
        "{:x}",
        sha2::Sha256::digest(serde_json::to_vec(&startup_recipe).unwrap())
    );
    request.driver_profile_version = "linux-x11-driver/v1".into();
    request.bounded_limits.timeout_seconds = 45;
    request.bounded_limits.max_screenshots = 2;
    request.bounded_limits.max_actions_per_scenario = 8;
    request.bounded_limits.max_total_actions = 8;
    request.validate().unwrap();

    let target = QaValidatedTargetV1 {
        task_uid: request.parent_task_uid.clone(),
        build_artifact_sha256: request.build_artifact_sha256.clone(),
        startup_recipe_ref: request.startup_recipe_ref.clone(),
        startup_recipe_sha256: request.startup_recipe_sha256.clone(),
        executable: "/workspace/approved-desktop-app".into(),
        working_directory: "/workspace".into(),
        arguments: Vec::new(),
    };
    target.validate_for(&request).unwrap();

    let scenario = QaScenarioV1 {
        schema_version: 1,
        id: uuid::Uuid::new_v4().hyphenated().to_string(),
        requirement_ids: request.requirement_ids.clone(),
        preconditions: vec!["The approved app artifact is staged in the task workspace".into()],
        steps: vec![
            step(QaDriverActionV1::LaunchValidatedTarget, None, false),
            step(QaDriverActionV1::WaitForWindow, Some("Egui:Ready"), true),
            step(
                QaDriverActionV1::Click {
                    window_id: "active".into(),
                    x: 100,
                    y: 36,
                },
                Some("clicked"),
                false,
            ),
            step(QaDriverActionV1::WaitForWindow, Some("Egui:Clicked"), false),
            step(
                QaDriverActionV1::Click {
                    window_id: "active".into(),
                    x: 100,
                    y: 88,
                },
                Some("clicked"),
                false,
            ),
            step(QaDriverActionV1::WaitForWindow, Some("Egui:Focused"), false),
            step(
                QaDriverActionV1::TypeText { text: "ok".into() },
                Some("typed 2 characters"),
                false,
            ),
            step(QaDriverActionV1::WaitForWindow, Some("Egui:Typed:ok"), true),
        ],
        expected_results: vec![
            "The approved application opens and responds to pointer and keyboard input".into(),
        ],
        negative_paths: Vec::new(),
        visual_expectations: vec!["Capture the initial and interacted application windows".into()],
        redaction_regions: Vec::new(),
        priority: 1,
    };
    scenario.validate(&request).unwrap();
    let request_file = serde_json::json!({
        "schema_version": 1,
        "request": request,
        "scenario": scenario,
        "target": target,
    });
    let request_upload =
        QaContainerUpload::request_file(&serde_json::to_vec(&request_file).unwrap(), 256 * 1024)
            .unwrap();
    let uploads = [artifact_upload, request_upload];
    let artifacts = [mount];
    let spec = QaDriverContainerSpec {
        owner_id: &request.execution_session_id,
        request: &request,
        task_uid: &request.parent_task_uid,
        driver: "desktop",
        image,
        artifacts: &artifacts,
        dependency_caches: &[],
        network: crate::execution::docker::qa_container::QaDriverNetwork::Disabled,
        service_lease: None,
        relay_binding: None,
        resources,
    };
    let body = spec.create_body_with_lock(lock).unwrap();
    let name = format!("koolade-qa-{}", request.qa_run_id);
    let (_cancel, mut cancel) = tokio::sync::watch::channel(false);
    let output = crate::execution::docker::qa_container::run_ephemeral_driver(
        docker,
        &name,
        body,
        &uploads,
        Duration::from_secs(90),
        1024 * 1024,
        &mut cancel,
    )
    .await
    .unwrap();
    assert_eq!(
        output.exit_code,
        0,
        "stdout={:?}, stderr={:?}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let response: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(response["completed"], true, "{response}");
    assert_eq!(response["failure"], serde_json::Value::Null, "{response}");
    assert_eq!(
        response["actions"].as_array().unwrap().len(),
        8,
        "{response}"
    );
    assert_eq!(
        response["screenshots"].as_array().unwrap().len(),
        2,
        "{response}"
    );
    let assertions = response["assertions"].as_array().unwrap();
    assert!(assertions.len() >= 5, "{response}");
    assert!(
        assertions
            .iter()
            .all(|assertion| assertion["status"] == "passed")
    );
    match docker.inspect_container(&name, None).await {
        Err(bollard::errors::Error::DockerResponseServerError {
            status_code: 404, ..
        }) => {}
        other => panic!("approved target QA container cleanup was not confirmed: {other:?}"),
    }
}

fn step(
    driver_action: QaDriverActionV1,
    expected_state: Option<&str>,
    screenshot_checkpoint: bool,
) -> QaScenarioStepV1 {
    QaScenarioStepV1 {
        driver_action,
        expected_state: expected_state.map(str::to_owned),
        screenshot_checkpoint,
    }
}

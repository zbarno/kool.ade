use std::{os::unix::fs::PermissionsExt, path::Path, time::Duration};

use crate::{
    execution::{
        contract::ResourceBudgetV1,
        docker::{
            images::{ImageLock, SelectedImage},
            qa_container::{QaContainerUpload, QaDriverContainerSpec},
        },
    },
    runtime_qa::{QaDriverActionV1, QaScenarioStepV1, QaScenarioV1},
};

// Production boundary: request preparation consumes persisted reviewed task,
// target approval, Build/Start receipts, and artifact hashes before the
// app-owned rootless desktop driver uploads and exercises the selected bytes.
// Unit tests cannot prove Docker upload, target process ownership, Xvfb input,
// evidence capture, and container cleanup work together. External dependency:
// rootless Docker with the candidate image. Setup cost: one fixture binary
// build and one short desktop run after the image build.
pub(super) async fn exercise(
    docker: &bollard::Docker,
    image: &SelectedImage,
    lock: &ImageLock,
    resources: &ResourceBudgetV1,
) {
    let fixture_path = match std::env::var_os("KOOLADE_QA_DESKTOP_FIXTURE_EXECUTABLE") {
        Some(path) => path,
        None if std::env::var_os("KOOLADE_QA_REQUIRE_TASK_ARTIFACT").is_some() => {
            panic!("the rootless candidate job requires its separately built desktop fixture")
        }
        None => return,
    };
    let fixture_path = Path::new(&fixture_path);
    let metadata = std::fs::symlink_metadata(fixture_path).unwrap();
    assert!(metadata.is_file() && !metadata.file_type().is_symlink());
    assert!(metadata.permissions().mode() & 0o111 != 0);

    let fixture =
        crate::app::root::test_runtime_qa::DesktopArtifactFixture::from_executable(fixture_path);
    let dispatch: crate::app::root::test_runtime_qa::RootlessDesktopDispatch =
        crate::app::root::test_runtime_qa::prepare_rootless_desktop_dispatch(
            &fixture.project,
            &fixture.task,
        )
        .expect("production reviewed task request and approved artifact dispatch");
    assert_eq!(
        dispatch.request.relevant_platforms,
        [crate::runtime_qa::QaPlatform::LinuxDesktop]
    );
    assert_eq!(
        dispatch.request.target_id.as_deref(),
        Some("qa-desktop-fixture-target")
    );
    assert_eq!(
        dispatch.request.code_head_sha,
        fixture.task.reviewed_code_head_sha.as_deref().unwrap()
    );
    assert_eq!(
        dispatch.target.executable,
        "/workspace/dist/approved-desktop-app"
    );
    assert_eq!(dispatch.target.arguments, ["--qa-fixture".to_owned()]);
    let request = dispatch.request;
    let target = dispatch.target;
    let artifact_upload = dispatch.artifact_upload;
    let mount = dispatch.artifact_mount;

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

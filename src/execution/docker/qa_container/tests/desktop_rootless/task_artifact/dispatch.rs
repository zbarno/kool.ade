use std::time::Duration;

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

pub(crate) async fn exercise_prepared_dispatch(
    docker: &bollard::Docker,
    image: &SelectedImage,
    lock: &ImageLock,
    resources: &ResourceBudgetV1,
    dispatch: crate::app::root::test_runtime_qa::RootlessDesktopDispatch,
) {
    run(docker, image, lock, resources, dispatch).await;
}

pub(super) async fn run(
    docker: &bollard::Docker,
    image: &SelectedImage,
    lock: &ImageLock,
    resources: &ResourceBudgetV1,
    dispatch: crate::app::root::test_runtime_qa::RootlessDesktopDispatch,
) {
    let request = dispatch.request;
    let request_file = serde_json::json!({
        "schema_version": 1,
        "request": request,
        "scenario": scenario(&request),
        "target": dispatch.target,
    });
    let request_upload =
        QaContainerUpload::request_file(&serde_json::to_vec(&request_file).unwrap(), 256 * 1024)
            .unwrap();
    let uploads = [dispatch.artifact_upload, request_upload];
    let artifacts = [dispatch.artifact_mount];
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

fn scenario(request: &crate::runtime_qa::QaRunRequestV1) -> QaScenarioV1 {
    let requirement = request.acceptance_criteria[0].clone();
    QaScenarioV1 {
        schema_version: 1,
        id: uuid::Uuid::new_v4().hyphenated().to_string(),
        requirement_ids: request.requirement_ids.clone(),
        preconditions: vec!["The approved app artifact is staged in the task workspace".into()],
        steps: vec![
            step(QaDriverActionV1::LaunchValidatedTarget, None, false),
            step(QaDriverActionV1::WaitForWindow, Some("Target:Ready"), true),
            step(
                QaDriverActionV1::Click {
                    window_id: "active".into(),
                    x: 100,
                    y: 36,
                },
                Some("clicked"),
                false,
            ),
            step(
                QaDriverActionV1::WaitForWindow,
                Some("Target:Clicked"),
                false,
            ),
            step(
                QaDriverActionV1::Click {
                    window_id: "active".into(),
                    x: 100,
                    y: 36,
                },
                Some("clicked"),
                false,
            ),
            step(
                QaDriverActionV1::WaitForWindow,
                Some("Target:Clicked"),
                false,
            ),
            step(
                QaDriverActionV1::TypeText { text: "ok".into() },
                Some("typed 2 characters"),
                false,
            ),
            step(
                QaDriverActionV1::WaitForWindow,
                Some("Target:Typed:ok"),
                true,
            ),
        ],
        expected_results: vec![requirement],
        negative_paths: Vec::new(),
        visual_expectations: vec!["Capture the initial and interacted application windows".into()],
        redaction_regions: Vec::new(),
        priority: 1,
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

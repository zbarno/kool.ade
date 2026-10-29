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

#[path = "scenarios/actions.rs"]
mod actions;
#[path = "scenarios/cancellation.rs"]
pub(super) mod cancellation;
pub(super) use actions::happy_steps;

pub(super) async fn exercise_cancellation(
    docker: &bollard::Docker,
    image: &SelectedImage,
    lock: &ImageLock,
    resources: &ResourceBudgetV1,
    executable: &Path,
) {
    cancellation::exercise(docker, image, lock, resources, executable).await;
}

pub(super) fn require_approved_executable(path: &Path) {
    let metadata = std::fs::symlink_metadata(path).unwrap();
    assert!(metadata.is_file() && !metadata.file_type().is_symlink());
    assert!(metadata.permissions().mode() & 0o111 != 0);
}

/// Drives one ephemeral rootless desktop driver run against an approved
/// non-Rust packaged artifact and returns the driver's JSON evidence envelope
/// after verifying transport success and owned-container cleanup.
pub(super) async fn run_scenario(
    docker: &bollard::Docker,
    image: &SelectedImage,
    lock: &ImageLock,
    resources: &ResourceBudgetV1,
    executable: &Path,
    steps: Vec<(QaDriverActionV1, Option<&'static str>, bool)>,
    label: &str,
) -> serde_json::Value {
    let fixture = crate::app::root::test_runtime_qa::DesktopArtifactFixture::from_executable_with_start_arguments(
        executable,
        vec!["--qa-fixture".into()],
    );
    let dispatch: crate::app::root::test_runtime_qa::RootlessDesktopDispatch =
        crate::app::root::test_runtime_qa::prepare_rootless_desktop_dispatch(
            &fixture.project,
            &fixture.task,
        )
        .expect("production reviewed task request and approved non-Rust artifact dispatch");
    assert_eq!(
        dispatch.request.target_id.as_deref(),
        Some("qa-desktop-fixture-target")
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
        preconditions: vec![
            "The approved non-Rust packaged artifact is staged in the task workspace".into(),
        ],
        steps: steps
            .into_iter()
            .map(|(driver_action, expected_state, screenshot_checkpoint)| {
                step(driver_action, expected_state, screenshot_checkpoint)
            })
            .collect(),
        expected_results: vec![format!(
            "the non-Rust packaged artifact exhibits the {label} scenario outcome"
        )],
        negative_paths: Vec::new(),
        visual_expectations: vec!["Capture scenario-checkpoint application windows".into()],
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
        Duration::from_secs(120),
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
    match docker.inspect_container(&name, None).await {
        Err(bollard::errors::Error::DockerResponseServerError {
            status_code: 404, ..
        }) => {}
        other => panic!("non-Rust artifact QA container cleanup was not confirmed: {other:?}"),
    }
    response
}

/// Crash path (4 steps): launch, wait for Ready, click the fatal region C with
/// a screenshot checkpoint, then a trailing no-op so the premature exit is
/// observed BETWEEN steps instead of after the loop.
pub(super) fn crash_steps() -> Vec<(QaDriverActionV1, Option<&'static str>, bool)> {
    vec![
        (QaDriverActionV1::LaunchValidatedTarget, None, false),
        (
            QaDriverActionV1::WaitForWindow,
            Some("koolade nonrust qa: ready"),
            true,
        ),
        (
            QaDriverActionV1::Click {
                window_id: "active".into(),
                x: 100,
                y: 136,
            },
            Some("clicked"),
            true,
        ),
        (QaDriverActionV1::ReadAppLogs, None, false),
    ]
}

pub(super) fn step(
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

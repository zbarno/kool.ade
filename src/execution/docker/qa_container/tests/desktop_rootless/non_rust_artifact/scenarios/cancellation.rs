use std::{path::Path, time::Duration};

use crate::{
    app::root::test_runtime_qa::{
        DesktopArtifactFixture, RootlessDesktopDispatch, prepare_rootless_desktop_dispatch,
    },
    execution::{
        contract::ResourceBudgetV1,
        docker::{
            images::{ImageLock, SelectedImage},
            qa_container::{QaContainerUpload, QaDriverContainerSpec},
        },
    },
    runtime_qa::{QaDriverActionV1, QaScenarioV1},
};

/// Production boundary: cancel a real desktop driver after its approved app
/// starts inside rootless Docker, while the scenario waits for an unreachable
/// window state. Unit tests cannot prove Docker exec cancellation removes the
/// owned driver and its running desktop process without affecting the host.
/// External dependency: rootless Docker and the pinned desktop candidate.
/// Setup cost: reuse the compiled C/Xlib fixture and launch one short run.
pub(super) async fn exercise(
    docker: &bollard::Docker,
    image: &SelectedImage,
    lock: &ImageLock,
    resources: &ResourceBudgetV1,
    executable: &Path,
) {
    let fixture = DesktopArtifactFixture::from_executable_with_start_arguments(
        executable,
        vec!["--qa-fixture".into()],
    );
    let dispatch: RootlessDesktopDispatch =
        prepare_rootless_desktop_dispatch(&fixture.project, &fixture.task)
            .expect("reviewed task and approved desktop artifact are required");
    let request = dispatch.request;
    let scenario = QaScenarioV1 {
        schema_version: 1,
        id: uuid::Uuid::new_v4().hyphenated().to_string(),
        requirement_ids: request.requirement_ids.clone(),
        preconditions: vec!["The approved desktop artifact is staged".into()],
        steps: vec![
            super::step(QaDriverActionV1::LaunchValidatedTarget, None, false),
            super::step(
                QaDriverActionV1::WaitForWindow,
                Some("unreachable cancellation sentinel"),
                false,
            ),
        ],
        expected_results: vec!["Cancellation stops an active desktop QA run".into()],
        negative_paths: vec!["Cancel while the driver waits for the window state".into()],
        visual_expectations: Vec::new(),
        redaction_regions: Vec::new(),
        priority: 1,
    };
    scenario.validate(&request).unwrap();
    let request_bytes = serde_json::to_vec(&serde_json::json!({
        "schema_version": 1,
        "request": request,
        "scenario": scenario,
        "target": dispatch.target,
    }))
    .unwrap();
    let request_upload = QaContainerUpload::request_file(&request_bytes, 256 * 1024).unwrap();
    let artifacts = [dispatch.artifact_mount];
    let uploads = [dispatch.artifact_upload, request_upload];
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
    let (cancel_sender, mut cancel) = tokio::sync::watch::channel(false);
    let run_docker = docker.clone();
    let run_name = name.clone();
    let mut run = tokio::spawn(async move {
        crate::execution::docker::qa_container::run_ephemeral_driver(
            &run_docker,
            &run_name,
            body,
            &uploads,
            Duration::from_secs(90),
            1024 * 1024,
            &mut cancel,
        )
        .await
    });

    if let Err(error) = wait_for_approved_target(docker, &name).await {
        let _ = cancel_sender.send(true);
        let _ = tokio::time::timeout(Duration::from_secs(15), &mut run).await;
        panic!("{error}");
    }
    tokio::time::sleep(Duration::from_millis(250)).await;
    cancel_sender.send(true).unwrap();
    let result = tokio::time::timeout(Duration::from_secs(15), &mut run)
        .await
        .expect("cancelled desktop driver must stop promptly")
        .expect("desktop cancellation task must not panic");
    let error = result.expect_err("cancelled desktop run must not report success");
    assert!(
        error.message.contains("QA driver was cancelled"),
        "{error:?}"
    );
    assert_container_removed(docker, &name).await;
}

async fn wait_for_approved_target(docker: &bollard::Docker, name: &str) -> Result<(), String> {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(30);
    loop {
        if let Ok(processes) = docker
            .top_processes(
                name,
                Some(
                    bollard::query_parameters::TopOptionsBuilder::default()
                        .ps_args("aux")
                        .build(),
                ),
            )
            .await
            && processes.processes.unwrap_or_default().iter().any(|row| {
                row.join(" ")
                    .contains("/workspace/dist/approved-desktop-app")
            })
        {
            return Ok(());
        }
        if tokio::time::Instant::now() >= deadline {
            return Err("approved desktop target did not start before cancellation".into());
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

async fn assert_container_removed(docker: &bollard::Docker, name: &str) {
    match docker.inspect_container(name, None).await {
        Err(bollard::errors::Error::DockerResponseServerError {
            status_code: 404, ..
        }) => {}
        other => panic!("cancelled desktop driver container remains: {other:?}"),
    }
}

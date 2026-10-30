use super::*;
use std::time::Duration;

/// Rootless integration boundary: kill the approved task preview while a real
/// browser driver is navigating to it. The run must report failed QA evidence,
/// never a pass, and cleanup must still remove the run-owned browser container
/// while leaving shared preview resources for their owning caller. Unit tests
/// cannot establish browser behavior across a live container exit. Dependency:
/// rootless Docker with the pinned browser and Python preview images. Setup cost:
/// one short browser run and one container kill in the existing candidate test.
#[allow(clippy::too_many_arguments)]
pub(super) async fn assert_preview_exit_fails_the_active_browser_run(
    docker: &bollard::Docker,
    request: &crate::runtime_qa::QaRunRequestV1,
    browser_image: &crate::execution::docker::images::SelectedImage,
    lock: &crate::execution::docker::images::ImageLock,
    artifact_root: &tempfile::TempDir,
    artifact_upload: &QaContainerUpload,
    preview: &super::super::QaWebPreview,
    resources: &crate::execution::contract::ResourceBudgetV1,
) -> Result<(), String> {
    let scenario = QaScenarioV1 {
        schema_version: 1,
        id: uuid::Uuid::new_v4().hyphenated().to_string(),
        requirement_ids: request.requirement_ids.clone(),
        preconditions: vec!["The approved Python preview is running.".into()],
        steps: vec![step(
            QaDriverActionV1::BrowserOpen {
                url: "http://qa-service/?defect=preview-exit".into(),
            },
            "Navigation reports the preview failure instead of passing",
            false,
        )],
        expected_results: vec!["The browser run records the preview process failure.".into()],
        negative_paths: vec!["The task preview process exits during navigation.".into()],
        visual_expectations: Vec::new(),
        redaction_regions: Vec::new(),
        priority: 1,
    };
    request.validate()?;
    scenario.validate(request)?;
    let browser_request = build_browser_request(request, &scenario)?;
    let request_upload = QaContainerUpload::request_file(&browser_request, 256 * 1024)
        .map_err(|error| error.message)?;
    let artifacts = [QaArtifactMount::inspect(artifact_root.path(), "/workspace")
        .map_err(|error| error.message)?];
    let uploads = [artifact_upload.clone(), request_upload];
    let mut relay_binding = preview.relay_binding.clone();
    let spec = QaDriverContainerSpec {
        owner_id: &request.execution_session_id,
        request,
        task_uid: &request.parent_task_uid,
        driver: "browser",
        image: browser_image,
        artifacts: &artifacts,
        dependency_caches: &[],
        network: QaDriverNetwork::InternalPreviewRelay {
            network_id: relay_binding.relay_browser_network_id.clone(),
            relay_container_id: relay_binding.relay_container_id.clone(),
        },
        service_lease: Some(&preview.service_lease),
        relay_binding: Some(&relay_binding),
        resources,
    };
    let body = spec
        .create_body_with_lock(lock)
        .map_err(|error| error.message)?;
    let name = format!("koolade-qa-{}", request.qa_run_id);
    let (cancel_sender, mut cancel) = tokio::sync::watch::channel(false);
    let driver_id = create_driver_container(
        docker,
        &name,
        body,
        &uploads,
        Duration::from_secs(40),
        16 * 1024 * 1024,
        &mut cancel,
    )
    .await
    .map_err(|error| error.message)?;
    relay_binding.browser_container_id = Some(driver_id.clone());

    let result = async {
        validate_browser_driver_network(docker, request, &relay_binding, &driver_id)
            .await
            .map_err(|error| error.message)?;
        let run_docker = docker.clone();
        let run_id = driver_id.clone();
        let mut run_cancel = cancel.clone();
        let mut runner = tokio::spawn(async move {
            run_created_driver_with_status(
                &run_docker,
                &run_id,
                Duration::from_secs(90),
                16 * 1024 * 1024,
                &mut run_cancel,
            )
            .await
        });
        if let Err(error) = wait_for_preview_exit_marker(
            docker,
            &preview.relay_binding.relay_container_id,
            &browser_image.runtime_user,
        )
        .await
        {
            let _ = cancel_sender.send(true);
            if tokio::time::timeout(Duration::from_secs(10), &mut runner)
                .await
                .is_err()
            {
                runner.abort();
            }
            return Err(error);
        }
        if docker
            .kill_container(
                &preview.service_lease.task_container_id,
                None::<bollard::query_parameters::KillContainerOptions>,
            )
            .await
            .is_err()
        {
            let _ = cancel_sender.send(true);
            if tokio::time::timeout(Duration::from_secs(10), &mut runner)
                .await
                .is_err()
            {
                runner.abort();
            }
            return Err("task preview container could not be killed for the failure probe".into());
        }
        let execution = match tokio::time::timeout(Duration::from_secs(40), &mut runner).await {
            Ok(Ok(Ok(output))) => output,
            Ok(Ok(Err(error))) => {
                return Err(format!(
                    "browser driver failed before reporting preview loss: {}",
                    error.message
                ));
            }
            Ok(Err(_)) => return Err("browser driver task panicked after preview loss".into()),
            Err(_) => {
                let _ = cancel_sender.send(true);
                runner.abort();
                let _ = tokio::time::timeout(Duration::from_secs(2), &mut runner).await;
                return Err(
                    "browser driver did not finish after the preview process exited".into(),
                );
            }
        };
        let report = parse_browser_output(&execution.stdout, request, &scenario)?;
        if report.assertions.is_empty()
            || report
                .assertions
                .iter()
                .all(|assertion| assertion.status == crate::runtime_qa::QaAssertionStatus::Passed)
        {
            return Err("browser run passed despite the task preview process exiting".into());
        }
        let task = docker
            .inspect_container(&preview.service_lease.task_container_id, None)
            .await
            .map_err(|_| "failed preview container disappeared before owner cleanup".to_owned())?;
        if task.state.and_then(|state| state.status)
            != Some(bollard::models::ContainerStateStatusEnum::EXITED)
        {
            return Err(
                "preview container was not observed exited during the failure probe".into(),
            );
        }
        if docker
            .inspect_container(&preview.relay_binding.relay_container_id, None)
            .await
            .is_err()
        {
            return Err("preview failure removed the shared relay container".into());
        }
        Ok(())
    }
    .await;
    let cleanup = cleanup_driver_container(docker, &driver_id)
        .await
        .map_err(|error| error.message);
    match (result, cleanup) {
        (Ok(()), Ok(())) => Ok(()),
        (Err(error), Ok(())) | (Ok(()), Err(error)) => Err(error),
        (Err(error), Err(cleanup)) => {
            Err(format!("{error}; browser driver cleanup failed: {cleanup}"))
        }
    }
}

async fn wait_for_preview_exit_marker(
    docker: &bollard::Docker,
    relay_container_id: &str,
    relay_runtime_user: &str,
) -> Result<(), String> {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(60);
    while tokio::time::Instant::now() < deadline {
        let probe = tokio::time::timeout(
            Duration::from_secs(3),
            super::probe::marker_is_active(
                docker,
                relay_container_id,
                relay_runtime_user,
                "/__preview_exit_started",
                "started",
            ),
        )
        .await;
        match probe {
            Ok(Ok(true)) => return Ok(()),
            Ok(Ok(false)) => {}
            Ok(Err(error)) => return Err(error),
            Err(_) => return Err("timed out probing the preview exit marker".into()),
        }
        tokio::time::sleep(Duration::from_millis(250)).await;
    }
    Err("browser never started the preview exit navigation".into())
}

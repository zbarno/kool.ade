use super::*;
use std::time::Duration;

/// Rootless integration boundary: kill the QA browser container while its real
/// Chromium session is blocked on a live preview response. The driver must
/// fail closed, and cleanup must remove that run-owned container while leaving
/// the caller-owned preview and relay alive. Unit tests cannot prove Docker's
/// container-exit behavior. Dependency: rootless Docker and the pinned browser
/// and Python preview images. Setup cost: one short run and one container kill.
#[allow(clippy::too_many_arguments)]
pub(super) async fn assert_driver_crash_fails_closed(
    docker: &bollard::Docker,
    request: &crate::runtime_qa::QaRunRequestV1,
    browser_image: &crate::execution::docker::images::SelectedImage,
    lock: &ImageLock,
    artifact_root: &tempfile::TempDir,
    artifact_upload: &QaContainerUpload,
    preview: &super::super::QaWebPreview,
    resources: &crate::execution::contract::ResourceBudgetV1,
) -> Result<(), String> {
    // Reuse the request whose preview lease is already running. The lease is
    // immutably bound to this QA run ID; only the scenario needs a fresh ID.
    let run_request = request.clone();
    let scenario = QaScenarioV1 {
        schema_version: 1,
        id: uuid::Uuid::new_v4().hyphenated().to_string(),
        requirement_ids: run_request.requirement_ids.clone(),
        preconditions: vec!["The approved Python preview is running.".into()],
        steps: vec![
            step(
                QaDriverActionV1::BrowserOpen {
                    url: "http://qa-service/?defect=parallel".into(),
                },
                "The browser is navigating to the task preview.",
                false,
            ),
            step(
                QaDriverActionV1::BrowserAssertText {
                    locator: QaBrowserLocatorV1::Role {
                        role: "heading".into(),
                        name: "Preview fixture".into(),
                    },
                    expected_text: "Preview fixture".into(),
                },
                "The preview heading is visible after navigation completes.",
                true,
            ),
        ],
        expected_results: vec!["A crashed browser run is not accepted as passing QA.".into()],
        negative_paths: vec!["The browser driver container exits during navigation.".into()],
        visual_expectations: Vec::new(),
        redaction_regions: Vec::new(),
        priority: 1,
    };
    run_request.validate()?;
    scenario.validate(&run_request)?;
    let browser_request = build_browser_request(&run_request, &scenario)?;
    let request_upload = QaContainerUpload::request_file(&browser_request, 256 * 1024)
        .map_err(|error| error.message)?;
    let artifacts = [QaArtifactMount::inspect(artifact_root.path(), "/workspace")
        .map_err(|error| error.message)?];
    let uploads = [artifact_upload.clone(), request_upload];
    let relay_binding = preview.relay_binding.clone();
    let spec = QaDriverContainerSpec {
        owner_id: &run_request.execution_session_id,
        request: &run_request,
        task_uid: &run_request.parent_task_uid,
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
    let (cancel_sender, mut cancel) = tokio::sync::watch::channel(false);
    let driver_id = create_driver_container(
        docker,
        &format!("koolade-qa-{}", run_request.qa_run_id),
        body,
        &uploads,
        Duration::from_secs(40),
        16 * 1024 * 1024,
        &mut cancel,
    )
    .await
    .map_err(|error| error.message)?;

    let result = async {
        let mut active_binding = relay_binding.clone();
        active_binding.browser_container_id = Some(driver_id.clone());
        validate_browser_driver_network(docker, &run_request, &active_binding, &driver_id)
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
        wait_for_active_request(
            docker,
            &relay_binding.relay_container_id,
            &browser_image.runtime_user,
        )
        .await?;
        docker
            .kill_container(
                &driver_id,
                None::<bollard::query_parameters::KillContainerOptions>,
            )
            .await
            .map_err(|_| {
                "browser QA container could not be killed for the crash probe".to_owned()
            })?;

        match tokio::time::timeout(Duration::from_secs(20), &mut runner).await {
            Ok(Ok(Err(_))) => {}
            Ok(Ok(Ok(output))) if output.exit_code != 0 => {}
            Ok(Ok(Ok(_))) => return Err("crashed browser driver returned a successful exit".into()),
            Ok(Err(_)) => return Err("browser driver runner panicked after container exit".into()),
            Err(_) => {
                let _ = cancel_sender.send(true);
                runner.abort();
                let _ = tokio::time::timeout(Duration::from_secs(2), &mut runner).await;
                return Err("browser driver runner hung after its container exited".into());
            }
        }

        let state = docker
            .inspect_container(&driver_id, None)
            .await
            .map_err(|_| "crashed browser container disappeared before owned cleanup".to_owned())?
            .state
            .and_then(|state| state.status);
        if state == Some(bollard::models::ContainerStateStatusEnum::RUNNING) {
            return Err("browser driver remained running after the crash probe".into());
        }
        for id in [
            preview.service_lease.task_container_id.as_str(),
            preview.relay_binding.relay_container_id.as_str(),
        ] {
            let state = docker
                .inspect_container(id, None)
                .await
                .map_err(|_| "browser crash removed a caller-owned preview resource".to_owned())?
                .state
                .and_then(|state| state.status);
            if state != Some(bollard::models::ContainerStateStatusEnum::RUNNING) {
                return Err("browser crash stopped a caller-owned preview resource".into());
            }
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
        (Err(error), Err(cleanup)) => Err(format!("{error}; driver cleanup failed: {cleanup}")),
    }
}

async fn wait_for_active_request(
    docker: &bollard::Docker,
    relay_container_id: &str,
    relay_runtime_user: &str,
) -> Result<(), String> {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(30);
    while tokio::time::Instant::now() < deadline {
        if super::probe::marker_is_active(
            docker,
            relay_container_id,
            relay_runtime_user,
            "/__parallel_active",
            "active",
        )
        .await?
        {
            return Ok(());
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    Err("browser never entered the blocked preview request".into())
}

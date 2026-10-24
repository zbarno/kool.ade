use super::*;
use marker::wait_for_cancel_marker;
use std::time::Duration;

#[path = "cancellation/marker.rs"]
mod marker;

/// Production boundary: a real browser driver is blocked in a task-service
/// navigation when cancellation arrives; the controller must stop waiting,
/// remove the owned driver container, and leave the task service for the
/// caller's normal cleanup. A unit test cannot prove Docker exec cancellation
/// and forced container removal stop a live browser. External dependency:
/// rootless Docker with the pinned browser image and Python preview. Setup cost:
/// one short driver launch plus a marker readback from the isolated task
/// container.
#[allow(clippy::too_many_arguments)]
pub(super) async fn assert_cancelled_browser_navigation_is_cleaned(
    docker: &bollard::Docker,
    request: &crate::runtime_qa::QaRunRequestV1,
    browser_image: &crate::execution::docker::images::SelectedImage,
    lock: &crate::execution::docker::images::ImageLock,
    artifact_root: &tempfile::TempDir,
    artifact_upload: &QaContainerUpload,
    preview: &super::super::super::QaWebPreview,
    resources: &crate::execution::contract::ResourceBudgetV1,
) -> Result<(), String> {
    let scenario = QaScenarioV1 {
        schema_version: 1,
        id: uuid::Uuid::new_v4().hyphenated().to_string(),
        requirement_ids: request.requirement_ids.clone(),
        preconditions: vec!["The approved Python preview is running.".into()],
        steps: vec![step(
            QaDriverActionV1::BrowserOpen {
                url: "http://qa-service/?defect=slow-cancel".into(),
            },
            "The slow task page is opening when cancellation arrives",
            false,
        )],
        expected_results: vec!["Cancellation stops the in-flight browser navigation.".into()],
        negative_paths: vec!["The QA operator cancels during navigation.".into()],
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
    let validation = validate_browser_driver_network(docker, request, &relay_binding, &driver_id)
        .await
        .map_err(|error| error.message);
    if let Err(error) = validation {
        let cleanup = cleanup_and_verify_driver(docker, &driver_id).await;
        return Err(match cleanup {
            Ok(()) => error,
            Err(cleanup) => format!("{error}; driver cleanup failed: {cleanup}"),
        });
    }

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
    let mut completed_before_marker = None;
    let marker = tokio::select! {
        marker = wait_for_cancel_marker(docker, &preview.service_lease.task_container_id) => marker,
        execution = &mut runner => {
            completed_before_marker = Some(execution);
            Err("browser driver finished before the delayed task page was observed".into())
        }
    };
    let _ = cancel_sender.send(true);
    let execution = match completed_before_marker {
        Some(execution) => Ok(execution),
        None => {
            let execution = tokio::time::timeout(Duration::from_secs(10), &mut runner).await;
            if execution.is_err() {
                runner.abort();
                let _ = tokio::time::timeout(Duration::from_secs(2), &mut runner).await;
            }
            execution
        }
    };
    let cleanup = cleanup_and_verify_driver(docker, &driver_id).await;
    let preview_resources = assert_preview_resources_remain(docker, preview).await;
    let mut failures = Vec::new();
    if let Err(error) = marker {
        failures.push(error);
    }
    if let Err(error) = cleanup {
        failures.push(format!("cancelled driver cleanup failed: {error}"));
    }
    if let Err(error) = preview_resources {
        failures.push(error);
    }
    if let Err(error) = assert_container_removed(docker, &name).await {
        failures.push(format!(
            "cancelled browser driver name still exists: {error}"
        ));
    }
    match execution {
        Err(_) => failures.push("cancelled browser execution did not stop in time".into()),
        Ok(Err(_)) => failures.push("cancelled browser execution task failed".into()),
        Ok(Ok(Err(error))) if error.message.contains("QA driver was cancelled") => {}
        Ok(Ok(Err(error))) => failures.push(format!(
            "browser run failed for another reason: {}",
            error.message
        )),
        Ok(Ok(Ok(output))) => {
            failures.push(format!(
                "browser run finished instead of observing cancellation: {}",
                describe_driver_completion(&output)
            ));
        }
    }
    if failures.is_empty() {
        Ok(())
    } else {
        Err(failures.join("; "))
    }
}

async fn cleanup_and_verify_driver(
    docker: &bollard::Docker,
    driver_id: &str,
) -> Result<(), String> {
    let cleanup = cleanup_driver_container(docker, driver_id)
        .await
        .map_err(|error| error.message);
    let retry = if cleanup.is_err() {
        Some(
            cleanup_driver_container(docker, driver_id)
                .await
                .map_err(|error| error.message),
        )
    } else {
        None
    };
    let removed = assert_container_removed(docker, driver_id).await;
    let mut failures = Vec::new();
    if let Err(error) = cleanup {
        failures.push(format!("cleanup failed: {error}"));
    }
    if let Some(Err(error)) = retry {
        failures.push(format!("cleanup retry failed: {error}"));
    }
    if let Err(error) = removed {
        failures.push(error);
    }
    if failures.is_empty() {
        Ok(())
    } else {
        Err(failures.join("; "))
    }
}

async fn assert_preview_resources_remain(
    docker: &bollard::Docker,
    preview: &super::super::QaWebPreview,
) -> Result<(), String> {
    let mut failures = Vec::new();
    for container_id in [
        &preview.service_lease.task_container_id,
        &preview.relay_binding.relay_container_id,
    ] {
        if docker.inspect_container(container_id, None).await.is_err() {
            failures.push("cancelling a browser run removed a shared preview container");
        }
    }
    for network_id in [
        &preview.service_lease.task_network_id,
        &preview.relay_binding.relay_browser_network_id,
    ] {
        if docker.inspect_network(network_id, None).await.is_err() {
            failures.push("cancelling a browser run removed a shared preview network");
        }
    }
    if failures.is_empty() {
        Ok(())
    } else {
        Err(failures.join("; "))
    }
}

async fn assert_container_removed(docker: &bollard::Docker, id: &str) -> Result<(), String> {
    match docker.inspect_container(id, None).await {
        Err(bollard::errors::Error::DockerResponseServerError {
            status_code: 404, ..
        }) => Ok(()),
        other => Err(format!(
            "run-owned container cleanup was not confirmed: {other:?}"
        )),
    }
}

fn describe_driver_completion(
    output: &crate::execution::docker::qa_container::DriverContainerOutput,
) -> String {
    let report = serde_json::from_slice::<serde_json::Value>(&output.stdout).ok();
    let outcome = report
        .as_ref()
        .and_then(|report| report.get("outcome"))
        .and_then(serde_json::Value::as_str)
        .unwrap_or("unparseable");
    let final_url = report
        .as_ref()
        .and_then(|report| report.get("actions"))
        .and_then(serde_json::Value::as_array)
        .and_then(|actions| actions.last())
        .and_then(|action| action.get("post_state"))
        .and_then(|state| state.get("url"))
        .and_then(serde_json::Value::as_str)
        .unwrap_or("unknown");
    let failure = report
        .as_ref()
        .and_then(|report| report.get("error"))
        .and_then(serde_json::Value::as_str)
        .map(|error| error.chars().take(256).collect::<String>())
        .unwrap_or_else(|| "none".into());
    format!(
        "exit_code={}, outcome={outcome}, final_url={final_url}, failure={failure}",
        output.exit_code
    )
}

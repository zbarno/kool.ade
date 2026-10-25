use super::super::*;
use super::assertions::assert_container_removed;

pub(super) struct BrowserDriverOutcome {
    pub driver_container_id: String,
    pub output: Result<crate::runtime_qa::QaDriverOutputV1, String>,
}

#[allow(clippy::too_many_arguments)]
pub(super) async fn run_browser_driver(
    docker: &bollard::Docker,
    request: &crate::runtime_qa::QaRunRequestV1,
    scenario: &QaScenarioV1,
    image: &crate::execution::docker::images::SelectedImage,
    lock: &ImageLock,
    artifact_root: &std::path::Path,
    artifact_upload: &QaContainerUpload,
    preview: &super::super::super::QaWebPreview,
    resources: &ResourceBudgetV1,
) -> Result<BrowserDriverOutcome, String> {
    let request_bytes = build_browser_request(request, scenario)?;
    let request_upload = QaContainerUpload::request_file(&request_bytes, 256 * 1024)
        .map_err(|error| error.message)?;
    let artifacts =
        [QaArtifactMount::inspect(artifact_root, "/workspace").map_err(|error| error.message)?];
    let uploads = [artifact_upload.clone(), request_upload];
    let spec = QaDriverContainerSpec {
        owner_id: &request.execution_session_id,
        request,
        task_uid: &request.parent_task_uid,
        driver: "browser",
        image,
        artifacts: &artifacts,
        dependency_caches: &[],
        network: QaDriverNetwork::InternalPreviewRelay {
            network_id: preview.relay_binding.relay_browser_network_id.clone(),
            relay_container_id: preview.relay_binding.relay_container_id.clone(),
        },
        service_lease: Some(&preview.service_lease),
        relay_binding: Some(&preview.relay_binding),
        resources,
    };
    let body = spec
        .create_body_with_lock(lock)
        .map_err(|error| error.message)?;
    let name = format!("koolade-qa-{}", request.qa_run_id);
    let (_cancel_sender, mut cancel) = tokio::sync::watch::channel(false);
    let driver_id = create_driver_container(
        docker,
        &name,
        body,
        &uploads,
        Duration::from_secs(60),
        16 * 1024 * 1024,
        &mut cancel,
    )
    .await
    .map_err(|error| error.message)?;
    let output: Result<crate::runtime_qa::QaDriverOutputV1, String> = async {
        let mut relay_binding = preview.relay_binding.clone();
        relay_binding.browser_container_id = Some(driver_id.clone());
        validate_browser_driver_network(docker, request, &relay_binding, &driver_id)
            .await
            .map_err(|error| error.message)?;
        let output = run_created_driver_with_status(
            docker,
            &driver_id,
            Duration::from_secs(60),
            16 * 1024 * 1024,
            &mut cancel,
        )
        .await
        .map_err(|error| error.message)?;
        if output.exit_code != 0 {
            return Err("concurrent browser driver exited unsuccessfully".into());
        }
        parse_browser_output(&output.stdout, request, scenario)
    }
    .await;
    Ok(BrowserDriverOutcome {
        driver_container_id: driver_id,
        output,
    })
}

pub(super) async fn cleanup_outcome(
    docker: &bollard::Docker,
    outcome: &Result<BrowserDriverOutcome, String>,
) -> Result<(), String> {
    let Ok(outcome) = outcome else {
        return Ok(());
    };
    let cleanup = cleanup_driver_container(docker, &outcome.driver_container_id)
        .await
        .map_err(|error| error.message);
    let retry = if cleanup.is_err() {
        Some(
            cleanup_driver_container(docker, &outcome.driver_container_id)
                .await
                .map_err(|error| error.message),
        )
    } else {
        None
    };
    let removed = assert_container_removed(docker, &outcome.driver_container_id).await;
    let mut failures = Vec::new();
    if let Err(error) = cleanup {
        failures.push(format!("driver cleanup failed: {error}"));
    }
    if let Some(Err(error)) = retry {
        failures.push(format!("driver cleanup retry failed: {error}"));
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

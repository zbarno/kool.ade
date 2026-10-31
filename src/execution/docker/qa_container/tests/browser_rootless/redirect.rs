use super::*;

#[path = "redirect/invalid_inputs.rs"]
mod invalid_inputs;

#[allow(clippy::too_many_arguments)]
pub(super) async fn assert_origin_escape_cases(
    docker: &bollard::Docker,
    request: &crate::runtime_qa::QaRunRequestV1,
    browser_image: &crate::execution::docker::images::SelectedImage,
    lock: &crate::execution::docker::images::ImageLock,
    artifact_root: &tempfile::TempDir,
    artifact_upload: &QaContainerUpload,
    preview: &super::super::super::QaWebPreview,
    resources: &crate::execution::contract::ResourceBudgetV1,
    cancel: &mut tokio::sync::watch::Receiver<bool>,
) -> Result<(), String> {
    for (case_name, url, policy_violation) in super::support::origin_escape_cases() {
        assert_origin_escape_blocked(
            docker,
            request,
            browser_image,
            lock,
            artifact_root,
            artifact_upload,
            preview,
            resources,
            cancel,
            case_name,
            url,
            policy_violation,
        )
        .await
        .map_err(|error| format!("{case_name} was not blocked: {error}"))?;
    }
    invalid_inputs::assert_invalid_browser_inputs_denied(
        docker,
        request,
        browser_image,
        lock,
        artifact_root,
        artifact_upload,
        preview,
        resources,
        cancel,
    )
    .await?;
    assert_sink_hits_zero(
        docker,
        &preview.relay_binding.relay_container_id,
        &browser_image.runtime_user,
    )
    .await
    .map_err(|error| format!("origin-escape sink audit failed: {error}"))?;
    Ok(())
}

/// Zero-contact evidence for the escape attempts above: the hostile sink
/// plant in the adjacent task service counts contacts per sentinel tag, and
/// the tags used by the escape cases must remain at zero. Reading the
/// counters from inside the relay container also proves the sink path is
/// reachable for an adjacent sandbox member, so zero is positive containment
/// evidence rather than a vacuous pass.
/// Rootless Docker boundary: a unit test cannot prove that no proxied
/// connection ever reached the neighbour. Cost: two read-only execs in the
/// already-running relay container.
pub(super) async fn assert_sink_hits_zero(
    docker: &bollard::Docker,
    relay_container_id: &str,
    relay_runtime_user: &str,
) -> Result<(), String> {
    for tag in ["adjacent", "rebind"] {
        let hits =
            super::probe::sink_hit_count(docker, relay_container_id, relay_runtime_user, tag)
                .await
                .map_err(|error| format!("{tag} sink counter could not be audited: {error}"))?;
        if hits != 0 {
            return Err(format!(
                "{tag} hostile sink was contacted {hits} times despite the browser origin policy"
            ));
        }
    }
    Ok(())
}

/// Rootless Docker integration boundary: a browser request outside the preview
/// origin is stopped by the app-owned browser driver and its container is
/// cleaned. Unit tests cannot prove Chromium's runtime route policy. It uses
/// the rootless daemon and candidate image from the sibling preview smoke.
#[allow(clippy::too_many_arguments)]
pub(super) async fn assert_origin_escape_blocked(
    docker: &bollard::Docker,
    request: &crate::runtime_qa::QaRunRequestV1,
    browser_image: &crate::execution::docker::images::SelectedImage,
    lock: &crate::execution::docker::images::ImageLock,
    artifact_root: &tempfile::TempDir,
    artifact_upload: &QaContainerUpload,
    preview: &super::super::super::QaWebPreview,
    resources: &crate::execution::contract::ResourceBudgetV1,
    cancel: &mut tokio::sync::watch::Receiver<bool>,
    case_name: &str,
    url: &str,
    expected_policy_violation: &str,
) -> Result<(), String> {
    let scenario = QaScenarioV1 {
        schema_version: 1,
        id: uuid::Uuid::new_v4().hyphenated().to_string(),
        requirement_ids: request.requirement_ids.clone(),
        preconditions: vec!["The approved Python preview is running.".into()],
        steps: vec![super::step(
            QaDriverActionV1::BrowserOpen { url: url.into() },
            &format!("{case_name} is blocked"),
            true,
        )],
        expected_results: vec!["The browser remains inside the approved preview origin.".into()],
        negative_paths: vec![format!("{case_name} is denied by browser origin policy.")],
        visual_expectations: vec![],
        redaction_regions: vec![],
        priority: 1,
    };
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
    let driver_id = create_driver_container(
        docker,
        &name,
        body,
        &uploads,
        Duration::from_secs(120),
        16 * 1024 * 1024,
        cancel,
    )
    .await
    .map_err(|error| error.message)?;
    relay_binding.browser_container_id = Some(driver_id.clone());
    let result = async {
        validate_browser_driver_network(docker, request, &relay_binding, &driver_id)
            .await
            .map_err(|error| error.message)?;
        let output = run_created_driver_with_status(
            docker,
            &driver_id,
            Duration::from_secs(120),
            16 * 1024 * 1024,
            cancel,
        )
        .await;
        let output = output.map_err(|error| error.message)?;
        if output.exit_code == 0 {
            return Err(format!("browser driver allowed {case_name}"));
        }
        let report: serde_json::Value = serde_json::from_slice(&output.stdout)
            .map_err(|_| format!("browser driver returned no failure report for {case_name}"))?;
        if report.get("outcome").and_then(serde_json::Value::as_str) != Some("failed") {
            return Err(format!(
                "browser driver returned an unexpected outcome for {case_name}"
            ));
        }
        let violation = report
            .get("policy_violation")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default();
        if !violation.contains(expected_policy_violation) {
            return Err(format!(
                "browser driver did not report the expected policy violation for {case_name}"
            ));
        }
        Ok(())
    }
    .await;
    let cleanup = cleanup_driver_container(docker, &driver_id)
        .await
        .map_err(|error| error.message);
    result.and(cleanup)
}

use super::*;

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
    assert_non_http_navigation_schemes_denied(
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
    Ok(())
}

/// Rootless Docker integration boundary: send a deliberately malformed driver
/// request around the controller's scenario validator and prove the pinned
/// driver rejects non-HTTP schemes before Chromium starts. The invariant is
/// that bypassing app-side validation cannot let `file:` or `javascript:`
/// navigation execute. A unit test cannot prove the built driver's runtime
/// policy. This needs a rootless Docker daemon, the candidate browser image,
/// and the existing preview/service fixture; setup cost is one extra short-lived
/// driver container per scheme in the existing rootless integration run.
#[allow(clippy::too_many_arguments)]
async fn assert_non_http_navigation_schemes_denied(
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
    for (case_name, unsafe_url) in [
        ("file navigation", "file:///etc/passwd"),
        ("javascript navigation", "javascript:alert(1)"),
    ] {
        let scenario = QaScenarioV1 {
            schema_version: 1,
            id: uuid::Uuid::new_v4().hyphenated().to_string(),
            requirement_ids: request.requirement_ids.clone(),
            preconditions: Vec::new(),
            steps: vec![step(
                QaDriverActionV1::BrowserOpen {
                    url: "http://qa-service/".into(),
                },
                "unsafe navigation is rejected",
                false,
            )],
            expected_results: vec!["The browser driver rejects unsafe schemes.".into()],
            negative_paths: vec![case_name.into()],
            visual_expectations: Vec::new(),
            redaction_regions: Vec::new(),
            priority: 1,
        };
        let approved = build_browser_request(request, &scenario)?;
        let mut malformed: serde_json::Value = serde_json::from_slice(&approved)
            .map_err(|_| "approved browser request could not be decoded".to_owned())?;
        malformed["actions"][0]["url"] = serde_json::Value::String(unsafe_url.into());
        let malformed = serde_json::to_vec(&malformed)
            .map_err(|_| "malformed browser request could not be encoded".to_owned())?;
        let request_upload = QaContainerUpload::request_file(&malformed, 256 * 1024)
            .map_err(|error| error.message)?;
        let artifacts = [QaArtifactMount::inspect(artifact_root.path(), "/workspace")
            .map_err(|error| error.message)?];
        let uploads = [artifact_upload.clone(), request_upload];
        let relay_binding = preview.relay_binding.clone();
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
        let mut binding = relay_binding;
        binding.browser_container_id = Some(driver_id.clone());
        let result = async {
            validate_browser_driver_network(docker, request, &binding, &driver_id)
                .await
                .map_err(|error| error.message)?;
            run_created_driver_with_status(
                docker,
                &driver_id,
                Duration::from_secs(120),
                16 * 1024 * 1024,
                cancel,
            )
            .await
            .map_err(|error| error.message)
        }
        .await;
        let cleanup = cleanup_driver_container(docker, &driver_id)
            .await
            .map_err(|error| error.message);
        let output = result?;
        cleanup?;
        if output.exit_code != 2
            || !output.stdout.is_empty()
            || !String::from_utf8_lossy(&output.stderr)
                .contains("action is not allowed by browser QA policy")
        {
            return Err(format!(
                "browser driver did not reject {case_name} before opening Chromium"
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

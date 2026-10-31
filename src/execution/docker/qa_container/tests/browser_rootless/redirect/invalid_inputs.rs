use super::*;

enum InvalidInput {
    Url(&'static str),
    Action(serde_json::Value),
}

/// Rootless Docker integration boundary: send deliberately malformed driver
/// requests around the controller's scenario validator and prove the pinned
/// driver rejects unsafe URLs, arbitrary command actions, package and
/// extension installation before Chromium starts. A unit test cannot prove
/// the built driver's runtime policy. This needs rootless Docker, the candidate
/// browser image, and the existing preview/service fixture; the three added
/// attack cases create three extra short-lived driver containers in the
/// existing rootless integration run.
#[allow(clippy::too_many_arguments)]
pub(super) async fn assert_invalid_browser_inputs_denied(
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
    let cases = [
        ("file navigation", InvalidInput::Url("file:///etc/passwd")),
        (
            "javascript navigation",
            InvalidInput::Url("javascript:alert(1)"),
        ),
        (
            "credential-bearing relay URL",
            InvalidInput::Url("http://qa-user:synthetic-secret@qa-preview:8080/"),
        ),
        (
            "agent shell escalation",
            InvalidInput::Action(serde_json::json!({
                "action": "run_command",
                "command": "touch /tmp/koolade-qa-shell-escalation-marker"
            })),
        ),
        (
            "browser extension installation",
            InvalidInput::Action(serde_json::json!({
                "action": "install_extension",
                "path": "/tmp/koolade-qa-unauthorized-extension"
            })),
        ),
        (
            "package installation",
            InvalidInput::Action(serde_json::json!({
                "action": "install_package",
                "manager": "apt",
                "package": "curl"
            })),
        ),
    ];

    for (case_name, invalid_input) in cases {
        let scenario = QaScenarioV1 {
            schema_version: 1,
            id: uuid::Uuid::new_v4().hyphenated().to_string(),
            requirement_ids: request.requirement_ids.clone(),
            preconditions: Vec::new(),
            steps: vec![step(
                QaDriverActionV1::BrowserOpen {
                    url: "http://qa-service/".into(),
                },
                "unsafe driver input is rejected",
                false,
            )],
            expected_results: vec!["The browser driver rejects unsafe inputs.".into()],
            negative_paths: vec![case_name.into()],
            visual_expectations: Vec::new(),
            redaction_regions: Vec::new(),
            priority: 1,
        };
        let approved = build_browser_request(request, &scenario)?;
        let mut malformed: serde_json::Value = serde_json::from_slice(&approved)
            .map_err(|_| "approved browser request could not be decoded".to_owned())?;
        match invalid_input {
            InvalidInput::Url(url) => {
                malformed["actions"][0]["url"] = serde_json::Value::String(url.into());
            }
            InvalidInput::Action(action) => malformed["actions"][0] = action,
        }
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

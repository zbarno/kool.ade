use super::*;

/// Rootless Docker boundary: a real browser at a mobile viewport reaches a
/// seeded same-origin wrong-destination defect, preserves visible page state,
/// and reports the typed URL assertion as failed with viewport-bound screenshots.
/// A request/parser unit test cannot prove Chromium observed the responsive
/// fixture. Uses the candidate image and running Python preview; one short
/// driver container is the setup cost.
#[allow(clippy::too_many_arguments)]
pub(super) async fn assert_seeded_wrong_destination_is_reported(
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
    let scenario = QaScenarioV1 {
        schema_version: 1,
        id: uuid::Uuid::new_v4().hyphenated().to_string(),
        requirement_ids: request.requirement_ids.clone(),
        preconditions: vec!["The approved Python preview is running.".into()],
        steps: vec![
            super::step(
                QaDriverActionV1::BrowserOpen {
                    url: "http://qa-service/?defect=wrong-action".into(),
                },
                "Seeded wrong-destination fixture is open",
                true,
            ),
            super::step(
                QaDriverActionV1::BrowserSetViewport {
                    width: 390,
                    height: 844,
                },
                "The preview is rendered at a mobile viewport",
                true,
            ),
            super::step(
                QaDriverActionV1::BrowserAssertText {
                    locator: QaBrowserLocatorV1::TestId {
                        test_id: "status".into(),
                    },
                    expected_text: "Ready".into(),
                },
                "The status remains visible at the mobile viewport",
                false,
            ),
            super::step(
                QaDriverActionV1::BrowserClick {
                    locator: QaBrowserLocatorV1::Role {
                        role: "button".into(),
                        name: "Open details".into(),
                    },
                },
                "Open details does not navigate away from the preview root",
                false,
            ),
            super::step(
                QaDriverActionV1::BrowserAssertUrl {
                    url: "http://qa-service/".into(),
                },
                "The interaction remains on the expected preview root",
                true,
            ),
        ],
        expected_results: vec![
            "The mobile preview keeps its status visible and Open details stays on the preview root."
                .into(),
        ],
        negative_paths: vec![],
        visual_expectations: vec!["The preview fits a 390 by 844 mobile viewport.".into()],
        redaction_regions: vec![],
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
        .await
        .map_err(|error| error.message)?;
        if output.exit_code == 0 {
            return Err("browser driver missed the seeded wrong-destination defect".into());
        }
        let report = parse_browser_output(&output.stdout, request, &scenario)?;
        if !report.completed
            || report.assertions.len() != 2
            || report.assertions[0].status != crate::runtime_qa::QaAssertionStatus::Passed
            || report.assertions[1].status != crate::runtime_qa::QaAssertionStatus::Failed
            || !report
                .screenshots
                .iter()
                .any(|screenshot| screenshot.width == 390 && screenshot.height == 844)
        {
            return Err(
                "browser driver did not capture the mobile viewport and failed wrong-destination assertion"
                    .into(),
            );
        }
        Ok(())
    }
    .await;
    let cleanup = cleanup_driver_container(docker, &driver_id)
        .await
        .map_err(|error| error.message);
    result.and(cleanup)
}

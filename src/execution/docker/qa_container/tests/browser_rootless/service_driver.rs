use super::*;

pub(super) fn required_for_candidate() -> bool {
    match std::env::var("KOOLADE_QA_REQUIRE_SERVICE_DRIVER").as_deref() {
        Ok("1") => true,
        Ok("0") | Err(std::env::VarError::NotPresent) => false,
        _ => panic!("KOOLADE_QA_REQUIRE_SERVICE_DRIVER must be 0 or 1"),
    }
}

/// Runs the headless API driver through the same app-owned preview lease and
/// restricted relay used by the browser application path.
#[allow(clippy::too_many_arguments)]
pub(super) async fn run(
    docker: &bollard::Docker,
    request: &crate::runtime_qa::QaRunRequestV1,
    scenario: &QaScenarioV1,
    image: &crate::execution::docker::images::SelectedImage,
    lock: &ImageLock,
    artifact_root: &std::path::Path,
    artifact_upload: &QaContainerUpload,
    preview: &crate::execution::docker::qa_container::QaWebPreview,
    resources: &ResourceBudgetV1,
    cancel: &mut tokio::sync::watch::Receiver<bool>,
) -> Result<crate::runtime_qa::QaDriverOutputV1, String> {
    let mut service_request = request.clone();
    service_request.relevant_platforms = vec![crate::runtime_qa::QaPlatform::HeadlessService];
    let mut service_scenario = scenario.clone();
    service_scenario.id = uuid::Uuid::new_v4().hyphenated().to_string();
    service_scenario.preconditions = vec!["The approved API service is running.".into()];
    service_scenario.steps = vec![step(
        QaDriverActionV1::ApiRequest {
            method: QaHttpMethodV1::Post,
            path: "/save".into(),
            json_body: Some(r#"{"message":"headless fixture input"}"#.into()),
            expected_status: 200,
            expected_body_contains: Some("\"saved\":true".into()),
        },
        "The approved API request is saved by the preview service",
        false,
    )];
    if request.test_data_fixture_ref.as_deref() == Some("go-headless-api-v1") {
        service_scenario.steps.push(step(
            QaDriverActionV1::ApiRequest {
                method: QaHttpMethodV1::Get,
                path: "/runner-protection".into(),
                json_body: None,
                expected_status: 200,
                expected_body_contains: Some("\"protected\":true".into()),
            },
            "The same-UID fixture process cannot read or truncate its sealed readiness runner",
            false,
        ));
    }
    service_scenario.expected_results = vec!["The save endpoint reports a successful save.".into()];
    service_scenario.visual_expectations.clear();
    service_request.validate()?;
    service_scenario.validate(&service_request)?;
    let request_bytes = build_service_request(&service_request, &service_scenario)?;
    let request_upload = QaContainerUpload::request_file(&request_bytes, 256 * 1024)
        .map_err(|error| error.message)?;
    let artifact =
        QaArtifactMount::inspect(artifact_root, "/workspace").map_err(|error| error.message)?;
    let uploads = [artifact_upload.clone(), request_upload];
    let network = QaDriverNetwork::InternalPreviewRelay {
        network_id: preview.relay_binding.relay_browser_network_id.clone(),
        relay_container_id: preview.relay_binding.relay_container_id.clone(),
    };
    let spec = QaDriverContainerSpec {
        owner_id: &service_request.execution_session_id,
        request: &service_request,
        task_uid: &service_request.parent_task_uid,
        driver: "service",
        image,
        artifacts: &[artifact],
        dependency_caches: &[],
        network,
        service_lease: Some(&preview.service_lease),
        relay_binding: Some(&preview.relay_binding),
        resources,
    };
    let body = spec
        .create_body_with_lock(lock)
        .map_err(|error| error.message)?;
    let name = format!("koolade-qa-{}", service_request.qa_run_id);
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
    let mut relay_binding = preview.relay_binding.clone();
    relay_binding.browser_container_id = Some(driver_id.clone());
    let result = async {
        crate::execution::docker::backend::validate_qa_driver_network(
            docker,
            spec.driver,
            &service_request,
            &relay_binding,
            &driver_id,
        )
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
        if output.exit_code != 0 {
            return Err(format!(
                "headless service driver exited with {}: {}",
                output.exit_code,
                bounded_driver_output(&output.stdout, &output.stderr)
            ));
        }
        parse_browser_output(&output.stdout, &service_request, &service_scenario)
    }
    .await;
    cleanup_driver_container(docker, &driver_id)
        .await
        .map_err(|error| error.message)?;
    result
}

fn bounded_driver_output(stdout: &[u8], stderr: &[u8]) -> String {
    const MAX_DIAGNOSTIC_BYTES: usize = 4096;
    let stdout = String::from_utf8_lossy(stdout);
    let stderr = String::from_utf8_lossy(stderr);
    format!(
        "stdout={:?}; stderr={:?}",
        stdout
            .chars()
            .take(MAX_DIAGNOSTIC_BYTES)
            .collect::<String>(),
        stderr
            .chars()
            .take(MAX_DIAGNOSTIC_BYTES)
            .collect::<String>()
    )
}

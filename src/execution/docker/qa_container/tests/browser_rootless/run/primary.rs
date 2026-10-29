use super::super::*;

pub(super) struct PrimaryDriverContext<'a> {
    pub docker: &'a bollard::Docker,
    pub request: &'a crate::runtime_qa::QaRunRequestV1,
    pub scenario: &'a QaScenarioV1,
    pub browser_image: &'a crate::execution::docker::images::SelectedImage,
    pub lock: &'a ImageLock,
    pub artifact_root: &'a std::path::Path,
    pub artifact_upload: &'a QaContainerUpload,
    pub preview: &'a crate::execution::docker::qa_container::QaWebPreview,
    pub resources: &'a ResourceBudgetV1,
    pub cancel: &'a mut tokio::sync::watch::Receiver<bool>,
}

pub(super) struct PrimaryDriverResult {
    pub driver_container_id: Option<String>,
    pub output: Result<crate::runtime_qa::QaDriverOutputV1, String>,
}

pub(super) async fn run(context: PrimaryDriverContext<'_>) -> Result<PrimaryDriverResult, String> {
    let artifacts = [
        QaArtifactMount::inspect(context.artifact_root, "/workspace")
            .map_err(|error| error.message)?,
    ];
    let request_bytes = build_browser_request(context.request, context.scenario)?;
    let request_upload = QaContainerUpload::request_file(&request_bytes, 256 * 1024)
        .map_err(|error| error.message)?;
    let uploads = [context.artifact_upload.clone(), request_upload];
    let spec = QaDriverContainerSpec {
        owner_id: &context.request.execution_session_id,
        request: context.request,
        task_uid: &context.request.parent_task_uid,
        driver: "browser",
        image: context.browser_image,
        artifacts: &artifacts,
        dependency_caches: &[],
        network: QaDriverNetwork::InternalPreviewRelay {
            network_id: context
                .preview
                .relay_binding
                .relay_browser_network_id
                .clone(),
            relay_container_id: context.preview.relay_binding.relay_container_id.clone(),
        },
        service_lease: Some(&context.preview.service_lease),
        relay_binding: Some(&context.preview.relay_binding),
        resources: context.resources,
    };
    let body = spec
        .create_body_with_lock(context.lock)
        .map_err(|error| error.message)?;
    let expected_env = body.env.clone().unwrap_or_default();
    let name = format!("koolade-qa-{}", context.request.qa_run_id);
    let driver_id = create_driver_container(
        context.docker,
        &name,
        body,
        &uploads,
        Duration::from_secs(120),
        16 * 1024 * 1024,
        context.cancel,
    )
    .await
    .map_err(|error| error.message)?;
    let result = async {
        let mut relay_binding = context.preview.relay_binding.clone();
        relay_binding.browser_container_id = Some(driver_id.clone());
        super::super::identity::assert_no_host_browser_identity(
            context.docker,
            &driver_id,
            &relay_binding.relay_token,
            &expected_env,
            &context.browser_image.reference,
        )
        .await?;
        validate_browser_driver_network(
            context.docker,
            context.request,
            &relay_binding,
            &driver_id,
        )
        .await
        .map_err(|error| error.message)?;
        let output = run_created_driver(
            context.docker,
            &driver_id,
            Duration::from_secs(120),
            16 * 1024 * 1024,
            context.cancel,
        )
        .await
        .map_err(|error| error.message)?;
        parse_browser_output(&output.stdout, context.request, context.scenario)
    }
    .await;
    Ok(PrimaryDriverResult {
        driver_container_id: Some(driver_id),
        output: result,
    })
}

use super::*;

#[allow(clippy::too_many_arguments)]
pub(super) fn run(
    docker: &bollard::Docker,
    lock: &ImageLock,
    browser_image: &crate::execution::docker::images::SelectedImage,
    service_image: &crate::execution::docker::images::SelectedImage,
    request: &crate::runtime_qa::QaRunRequestV1,
    scenario: &QaScenarioV1,
    recipe: &crate::project_runtime::ExecutionRecipeV1,
    artifact_upload: &QaContainerUpload,
    artifact_root: &Path,
    evidence_root: &Path,
) {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    runtime.block_on(run_async(
        docker,
        lock,
        browser_image,
        service_image,
        request,
        scenario,
        recipe,
        artifact_upload,
        artifact_root,
        evidence_root,
    ));
}

#[allow(clippy::too_many_arguments)]
async fn run_async(
    docker: &bollard::Docker,
    lock: &ImageLock,
    browser_image: &crate::execution::docker::images::SelectedImage,
    service_image: &crate::execution::docker::images::SelectedImage,
    request: &crate::runtime_qa::QaRunRequestV1,
    scenario: &QaScenarioV1,
    recipe: &crate::project_runtime::ExecutionRecipeV1,
    artifact_upload: &QaContainerUpload,
    artifact_root: &Path,
    evidence_root: &Path,
) {
    request.validate().unwrap();
    scenario.validate(request).unwrap();
    let mut resources = resources();
    resources.process_count = 128;
    let (_cancel_sender, mut cancel) = tokio::sync::watch::channel(false);
    let preview = super::super::super::super::service::start_web_preview_with_lock(
        docker,
        QaWebPreviewSpec {
            request,
            browser_image,
            service_image,
            recipe,
            artifact_upload,
            dependency_caches: &[],
            preview_port: 4173,
            health_path: "/health",
            resources: &resources,
        },
        &mut cancel,
        lock,
    )
    .await
    .unwrap();
    let result = async {
        validate_preview_relay_network(
            docker,
            request,
            &preview.service_lease,
            &preview.relay_binding,
        )
        .await
        .map_err(|error| error.message)?;
        super::super::super::service_driver::run(
            docker,
            request,
            scenario,
            browser_image,
            lock,
            artifact_root,
            artifact_upload,
            &preview,
            &resources,
            &mut cancel,
        )
        .await
    }
    .await;
    cleanup_web_preview(docker, &preview.service_lease, &preview.relay_binding)
        .await
        .unwrap();
    let output = result.unwrap();
    assert!(output.completed);
    assert_eq!(output.assertions.len(), scenario.steps.len());
    assert!(
        output
            .assertions
            .iter()
            .all(|assertion| assertion.status == crate::runtime_qa::QaAssertionStatus::Passed)
    );
    assert!(output.screenshots.is_empty());
    super::super::super::lifecycle::review_and_persist(
        request,
        scenario.clone(),
        &preview.service_lease,
        output,
        evidence_root.to_path_buf(),
    );
}

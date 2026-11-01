use super::*;

#[allow(clippy::too_many_arguments)]
pub(super) async fn assert(
    docker: &bollard::Docker,
    request: &crate::runtime_qa::QaRunRequestV1,
    browser_image: &crate::execution::docker::images::SelectedImage,
    lock: &crate::execution::docker::images::ImageLock,
    artifact_root: &tempfile::TempDir,
    artifact_upload: &QaContainerUpload,
    preview: &super::super::super::QaWebPreview,
    resources: &crate::execution::contract::ResourceBudgetV1,
) -> Result<(), String> {
    super::super::driver_crash::assert_driver_crash_fails_closed(
        docker,
        request,
        browser_image,
        lock,
        artifact_root,
        artifact_upload,
        preview,
        resources,
    )
    .await?;
    super::super::preview_failure::assert_preview_exit_fails_the_active_browser_run(
        docker,
        request,
        browser_image,
        lock,
        artifact_root,
        artifact_upload,
        preview,
        resources,
    )
    .await
}

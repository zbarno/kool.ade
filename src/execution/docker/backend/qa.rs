use super::{DockerExecutionBackend, internal_error_with_context};
use crate::execution::{
    docker::qa_container::{self, QaWebPreview, QaWebPreviewSpec},
    errors::ExecutionError,
};

pub(super) async fn start(
    backend: &DockerExecutionBackend,
    spec: QaWebPreviewSpec<'_>,
    cancel: &mut tokio::sync::watch::Receiver<bool>,
) -> Result<QaWebPreview, ExecutionError> {
    let image = backend
        .docker
        .inspect_image(&spec.browser_image.reference)
        .await
        .map_err(|_| internal_error_with_context("inspecting the locked browser QA image"))?;
    let observation = crate::execution::docker::images::ImageObservation {
        image_id: image.id.unwrap_or_default(),
        repo_digests: image.repo_digests.unwrap_or_default(),
        os: image.os.unwrap_or_default(),
        architecture: image.architecture.unwrap_or_default(),
        configured_user: image
            .config
            .as_ref()
            .and_then(|config| config.user.clone())
            .unwrap_or_default(),
        labels: image
            .config
            .and_then(|config| config.labels)
            .unwrap_or_default()
            .into_iter()
            .collect(),
    };
    crate::execution::docker::images::verify_qa_image_observation(spec.browser_image, &observation)
        .map_err(|_| {
            internal_error_with_context("verifying the locked browser QA digest and user")
        })?;
    qa_container::start_web_preview(&backend.docker, spec, cancel).await
}

pub(super) async fn cleanup(
    backend: &DockerExecutionBackend,
    lease: &crate::runtime_qa::QaTaskServiceLeaseV1,
    binding: &crate::runtime_qa::QaPreviewRelayBindingV1,
) -> Result<(), ExecutionError> {
    if lease.task_network_id.is_empty() || binding.relay_browser_network_id.is_empty() {
        return Err(internal_error_with_context(
            "validating the QA preview lease and relay binding",
        ));
    }
    qa_container::cleanup_web_preview(&backend.docker, lease, binding).await
}

pub(super) async fn recover(
    backend: &DockerExecutionBackend,
    request: &crate::runtime_qa::QaRunRequestV1,
) -> Result<(), ExecutionError> {
    qa_container::recover_owned_web_preview(&backend.docker, request).await
}

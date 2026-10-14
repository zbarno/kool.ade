use super::{DockerExecutionBackend, internal_error};
use crate::execution::{
    docker::qa_container::{self, QaWebPreview, QaWebPreviewSpec},
    errors::ExecutionError,
};

pub(super) async fn start(
    backend: &DockerExecutionBackend,
    spec: QaWebPreviewSpec<'_>,
    cancel: &mut tokio::sync::watch::Receiver<bool>,
) -> Result<QaWebPreview, ExecutionError> {
    qa_container::start_web_preview(&backend.docker, spec, cancel).await
}

pub(super) async fn cleanup(
    backend: &DockerExecutionBackend,
    lease: &crate::runtime_qa::QaTaskServiceLeaseV1,
    binding: &crate::runtime_qa::QaPreviewRelayBindingV1,
) -> Result<(), ExecutionError> {
    if lease.task_network_id.is_empty() || binding.relay_browser_network_id.is_empty() {
        return Err(internal_error());
    }
    qa_container::cleanup_web_preview(&backend.docker, lease, binding).await
}

pub(super) async fn recover(
    backend: &DockerExecutionBackend,
    request: &crate::runtime_qa::QaRunRequestV1,
) -> Result<(), ExecutionError> {
    qa_container::recover_owned_web_preview(&backend.docker, request).await
}

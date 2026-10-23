use crate::{
    execution::errors::{ExecutionError, ExecutionErrorKind},
    runtime_qa::{QaPreviewRelayBindingV1, QaRunRequestV1},
};

/// Dispatch QA network validation by the fixed driver type selected by policy.
pub(crate) async fn validate_qa_driver_network(
    docker: &bollard::Docker,
    driver: &str,
    request: &QaRunRequestV1,
    binding: &QaPreviewRelayBindingV1,
    container_id: &str,
) -> Result<(), ExecutionError> {
    match driver {
        "browser" => {
            crate::execution::docker::qa_container::validate_browser_driver_network(
                docker,
                request,
                binding,
                container_id,
            )
            .await
        }
        "service" => {
            crate::execution::docker::qa_container::validate_service_driver_network(
                docker,
                request,
                binding,
                container_id,
            )
            .await
        }
        _ => Err(ExecutionError::new(
            ExecutionErrorKind::PolicyDenied,
            "QA driver network binding is not approved",
            false,
        )),
    }
}

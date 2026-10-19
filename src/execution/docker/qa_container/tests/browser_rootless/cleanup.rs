use super::*;

/// Confirm the app removes only the driver, preview, relay, and networks it
/// owns after both a passing run and a blocked redirect attempt.
pub(super) async fn cleanup_and_assert(
    docker: &bollard::Docker,
    request: &crate::runtime_qa::QaRunRequestV1,
    driver_id: Option<String>,
    preview: &super::super::super::QaWebPreview,
) {
    if let Some(driver_id) = driver_id {
        cleanup_driver_container(docker, &driver_id).await.unwrap();
    }
    cleanup_web_preview(docker, &preview.service_lease, &preview.relay_binding)
        .await
        .unwrap();
    assert!(matches!(
        docker
            .inspect_container(&format!("koolade-qa-{}", request.qa_run_id), None)
            .await,
        Err(bollard::errors::Error::DockerResponseServerError {
            status_code: 404,
            ..
        })
    ));
    for id in [
        preview.service_lease.task_container_id.as_str(),
        preview.relay_binding.relay_container_id.as_str(),
    ] {
        assert!(matches!(
            docker.inspect_container(id, None).await,
            Err(bollard::errors::Error::DockerResponseServerError {
                status_code: 404,
                ..
            })
        ));
    }
    for id in [
        preview.service_lease.task_network_id.as_str(),
        preview.relay_binding.relay_browser_network_id.as_str(),
    ] {
        assert!(matches!(
            docker.inspect_network(id, None).await,
            Err(bollard::errors::Error::DockerResponseServerError {
                status_code: 404,
                ..
            })
        ));
    }
}

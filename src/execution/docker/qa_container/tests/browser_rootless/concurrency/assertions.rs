use std::time::Duration;

pub(super) async fn wait_for_simultaneous_parallel_requests(
    docker: &bollard::Docker,
    first_relay_container_id: &str,
    second_relay_container_id: &str,
    relay_runtime_user: &str,
    timeout: Duration,
) -> Result<(), String> {
    let deadline = tokio::time::Instant::now() + timeout;
    while tokio::time::Instant::now() < deadline {
        let active_markers = tokio::time::timeout(Duration::from_secs(5), async {
            tokio::join!(
                super::super::probe::marker_is_active(
                    docker,
                    first_relay_container_id,
                    relay_runtime_user,
                    "/__parallel_active",
                    "active",
                ),
                super::super::probe::marker_is_active(
                    docker,
                    second_relay_container_id,
                    relay_runtime_user,
                    "/__parallel_active",
                    "active",
                )
            )
        })
        .await
        .map_err(|_| "timed out reading parallel request markers".to_string())?;
        let (first_active, second_active) = active_markers;
        if first_active? && second_active? {
            return Ok(());
        }
        tokio::time::sleep(Duration::from_millis(250)).await;
    }
    Err("the two browser requests were never active at the same time".into())
}

pub(super) async fn assert_run_resources_removed(
    docker: &bollard::Docker,
    preview: &super::super::super::super::QaWebPreview,
) -> Result<(), String> {
    let mut failures = Vec::new();
    for container_id in [
        &preview.service_lease.task_container_id,
        &preview.relay_binding.relay_container_id,
    ] {
        if let Err(error) = assert_container_removed(docker, container_id).await {
            failures.push(error);
        }
    }
    for network_id in [
        &preview.service_lease.task_network_id,
        &preview.relay_binding.relay_browser_network_id,
    ] {
        match docker.inspect_network(network_id, None).await {
            Err(bollard::errors::Error::DockerResponseServerError {
                status_code: 404, ..
            }) => {}
            other => failures.push(format!(
                "run-owned network cleanup was not confirmed: {other:?}"
            )),
        }
    }
    if failures.is_empty() {
        Ok(())
    } else {
        Err(failures.join("; "))
    }
}

pub(super) async fn assert_run_resources_present(
    docker: &bollard::Docker,
    preview: &super::super::super::super::QaWebPreview,
) -> Result<(), String> {
    let mut failures = Vec::new();
    for container_id in [
        &preview.service_lease.task_container_id,
        &preview.relay_binding.relay_container_id,
    ] {
        if docker.inspect_container(container_id, None).await.is_err() {
            failures.push("cleanup for one browser run removed another run's container");
        }
    }
    for network_id in [
        &preview.service_lease.task_network_id,
        &preview.relay_binding.relay_browser_network_id,
    ] {
        if docker.inspect_network(network_id, None).await.is_err() {
            failures.push("cleanup for one browser run removed another run's network");
        }
    }
    if failures.is_empty() {
        Ok(())
    } else {
        Err(failures.join("; "))
    }
}

pub(super) async fn assert_container_removed(
    docker: &bollard::Docker,
    id: &str,
) -> Result<(), String> {
    match docker.inspect_container(id, None).await {
        Err(bollard::errors::Error::DockerResponseServerError {
            status_code: 404, ..
        }) => Ok(()),
        other => Err(format!(
            "run-owned container cleanup was not confirmed: {other:?}"
        )),
    }
}

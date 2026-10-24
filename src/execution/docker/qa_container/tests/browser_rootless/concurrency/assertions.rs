use futures_util::StreamExt;
use std::{io::Read, time::Duration};

const PARALLEL_MARKER: &str = "/tmp/qa-browser-parallel-active";

pub(super) async fn wait_for_simultaneous_parallel_requests(
    docker: &bollard::Docker,
    first_task_container_id: &str,
    second_task_container_id: &str,
    timeout: Duration,
) -> Result<(), String> {
    let deadline = tokio::time::Instant::now() + timeout;
    while tokio::time::Instant::now() < deadline {
        let active_markers = tokio::time::timeout(Duration::from_secs(5), async {
            tokio::join!(
                parallel_request_is_active(docker, first_task_container_id),
                parallel_request_is_active(docker, second_task_container_id)
            )
        })
        .await
        .map_err(|_| "timed out reading parallel request markers".to_string())?;
        let (first_active, second_active) = active_markers;
        if first_active? && second_active? {
            return Ok(());
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    Err("the two browser requests were never active at the same time".into())
}

async fn parallel_request_is_active(
    docker: &bollard::Docker,
    task_container_id: &str,
) -> Result<bool, String> {
    let options = bollard::query_parameters::DownloadFromContainerOptionsBuilder::default()
        .path(PARALLEL_MARKER)
        .build();
    let mut archive_stream = docker.download_from_container(task_container_id, Some(options));
    let mut bytes = Vec::new();
    while let Some(chunk) = archive_stream.next().await {
        match chunk {
            Ok(chunk) => bytes.extend_from_slice(&chunk),
            Err(_) => return Ok(false),
        }
    }
    Ok(archive_contains_active_marker(&bytes))
}

fn archive_contains_active_marker(bytes: &[u8]) -> bool {
    let mut archive = tar::Archive::new(bytes);
    let Ok(entries) = archive.entries() else {
        return false;
    };
    entries.flatten().any(|mut entry| {
        let is_marker = entry
            .path()
            .map(|path| {
                path.file_name().and_then(std::ffi::OsStr::to_str)
                    == Some("qa-browser-parallel-active")
            })
            .unwrap_or(false);
        if !is_marker {
            return false;
        }
        let mut content = String::new();
        entry.read_to_string(&mut content).is_ok() && content == "active"
    })
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

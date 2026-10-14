use std::time::Duration;

use bollard::query_parameters::InspectContainerOptionsBuilder;

use crate::execution::docker::backend::DockerExecutionBackend;

pub(super) async fn cleanup_created_container(
    backend: &DockerExecutionBackend,
    container_id: &str,
    execution_id: &str,
) {
    if !matches!(
        tokio::time::timeout(
            Duration::from_secs(5),
            super::super::control::verify_owned(backend, container_id, execution_id),
        )
        .await,
        Ok(Ok(()))
    ) {
        return;
    }
    let stopped = tokio::time::timeout(
        Duration::from_secs(2),
        backend.docker.stop_container(
            container_id,
            Some(bollard::query_parameters::StopContainerOptions {
                t: Some(1),
                ..Default::default()
            }),
        ),
    )
    .await
    .is_ok_and(|result| result.is_ok());
    if !stopped {
        let _ = tokio::time::timeout(
            Duration::from_secs(2),
            backend.docker.kill_container(
                container_id,
                None::<bollard::query_parameters::KillContainerOptions>,
            ),
        )
        .await;
    }
    let _ = tokio::time::timeout(
        Duration::from_secs(3),
        backend.docker.remove_container(
            container_id,
            Some(bollard::query_parameters::RemoveContainerOptions {
                force: true,
                ..Default::default()
            }),
        ),
    )
    .await;
}

pub(super) async fn cleanup_named_container(
    backend: &DockerExecutionBackend,
    name: &str,
    execution_id: &str,
) {
    let inspected = tokio::time::timeout(
        Duration::from_secs(3),
        backend.docker.inspect_container(
            name,
            Some(
                InspectContainerOptionsBuilder::default()
                    .size(false)
                    .build(),
            ),
        ),
    )
    .await;
    let Ok(Ok(info)) = inspected else {
        return;
    };
    let labels = info
        .config
        .and_then(|config| config.labels)
        .unwrap_or_default();
    if labels
        .get(crate::execution::docker::sandbox::OWNER_LABEL)
        .map(String::as_str)
        != Some(&backend.identity.owner_instance_id)
        || labels
            .get(crate::execution::docker::sandbox::EXECUTION_LABEL)
            .map(String::as_str)
            != Some(execution_id)
    {
        return;
    }
    let _ = tokio::time::timeout(
        Duration::from_secs(3),
        backend.docker.remove_container(
            name,
            Some(bollard::query_parameters::RemoveContainerOptions {
                force: true,
                ..Default::default()
            }),
        ),
    )
    .await;
}

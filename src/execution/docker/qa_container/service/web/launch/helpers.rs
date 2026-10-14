use bollard::{Docker, query_parameters::RemoveContainerOptionsBuilder};
use std::time::{Duration, Instant};

use super::super::super::super::denied;
use super::super::labels;
use crate::execution::errors::ExecutionError;

pub(super) async fn wait_healthy(
    docker: &Docker,
    id: &str,
    timeout: Duration,
    cancel: &mut tokio::sync::watch::Receiver<bool>,
) -> Result<(), ExecutionError> {
    let until = Instant::now() + timeout;
    loop {
        if *cancel.borrow() {
            return Err(denied("QA preview readiness was cancelled"));
        }
        let inspect = docker
            .inspect_container(id, None)
            .await
            .map_err(|_| denied("QA relay readiness could not be inspected"))?;
        let state = inspect.state.as_ref();
        if state.is_some_and(|state| {
            state.status == Some(bollard::models::ContainerStateStatusEnum::DEAD)
                || state.status == Some(bollard::models::ContainerStateStatusEnum::EXITED)
        }) {
            return Err(denied(
                "QA preview relay exited before the approved service readiness probe passed",
            ));
        }
        if state
            .and_then(|state| state.health.as_ref())
            .and_then(|health| health.status)
            == Some(bollard::models::HealthStatusEnum::HEALTHY)
        {
            return Ok(());
        }
        if Instant::now() >= until {
            return Err(denied(
                "Approved task service did not pass its exact port/path readiness probe before timeout",
            ));
        }
        tokio::time::sleep(Duration::from_millis(250)).await;
    }
}

pub(super) async fn remove_named(
    docker: &Docker,
    name: &str,
    request: &crate::runtime_qa::QaRunRequestV1,
    role: &str,
) {
    let Ok(container) = docker.inspect_container(name, None).await else {
        return;
    };
    let container_labels = container
        .config
        .and_then(|config| config.labels)
        .unwrap_or_default();
    if container_labels != labels(request, role) {
        return;
    }
    let _ = docker
        .remove_container(
            name,
            Some(
                RemoveContainerOptionsBuilder::default()
                    .force(true)
                    .v(false)
                    .build(),
            ),
        )
        .await;
}

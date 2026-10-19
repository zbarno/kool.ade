use bollard::{
    Docker, models::ContainerCreateBody, query_parameters::CreateContainerOptionsBuilder,
};

use super::super::super::{stage_qa_uploads, start_detached_exec};
use super::super::{connect_request, readiness_timeout, relay_body, service_body};
use super::helpers::wait_healthy;
use crate::execution::{
    contract::DependencyCacheEcosystemV1,
    docker::qa_container::{QaContainerUpload, QaWebPreviewSpec, denied},
    errors::ExecutionError,
};

pub(super) async fn launch_containers(
    docker: &Docker,
    spec: &QaWebPreviewSpec<'_>,
    cancel: &mut tokio::sync::watch::Receiver<bool>,
    task_network: &str,
    browser_network: &str,
    task_name: &str,
    relay_name: &str,
) -> Result<(String, String), ExecutionError> {
    let mut body = service_body(spec, task_network)?;
    let command = body
        .entrypoint
        .take()
        .unwrap_or_default()
        .into_iter()
        .chain(body.cmd.take().unwrap_or_default())
        .collect::<Vec<_>>();
    if command.is_empty() {
        return Err(denied("Approved QA task service command is empty"));
    }
    body.entrypoint = Some(vec!["/usr/bin/sleep".into()]);
    body.cmd = Some(vec!["infinity".into()]);
    let task_id = docker
        .create_container(
            Some(
                CreateContainerOptionsBuilder::default()
                    .name(task_name)
                    .build(),
            ),
            body,
        )
        .await
        .map_err(|_| denied("Approved QA task service container could not be created"))?
        .id;
    docker
        .start_container(&task_id, None)
        .await
        .map_err(|_| denied("Approved QA task service staging container could not start"))?;

    let mut uploads = vec![spec.artifact_upload.clone()];
    for cache in spec.dependency_caches {
        let target = match cache.cache_ref.ecosystem {
            DependencyCacheEcosystemV1::Npm => "/home/executor/.npm",
            DependencyCacheEcosystemV1::Cargo => "/home/executor/.cargo",
        };
        uploads.push(QaContainerUpload {
            target: target.into(),
            archive: super::super::dependency_cache::archive_for_runtime_user(
                cache,
                &spec.service_image.runtime_user,
            )?,
        });
    }
    stage_qa_uploads(
        docker,
        &task_id,
        &uploads,
        readiness_timeout(spec.recipe),
        cancel,
    )
    .await?;
    if *cancel.borrow() {
        return Err(denied("QA web preview was cancelled before service start"));
    }
    start_detached_exec(
        docker,
        &task_id,
        command,
        &spec.service_image.runtime_user,
        readiness_timeout(spec.recipe),
        cancel,
    )
    .await?;

    let relay_id = start_preview_relay(
        docker,
        spec,
        task_network,
        browser_network,
        relay_name,
        cancel,
    )
    .await?;
    Ok((task_id, relay_id))
}

async fn start_preview_relay(
    docker: &Docker,
    spec: &QaWebPreviewSpec<'_>,
    task_network: &str,
    browser_network: &str,
    relay_name: &str,
    cancel: &mut tokio::sync::watch::Receiver<bool>,
) -> Result<String, ExecutionError> {
    let body: ContainerCreateBody = relay_body(spec, task_network);
    let relay_id = docker
        .create_container(
            Some(
                CreateContainerOptionsBuilder::default()
                    .name(relay_name)
                    .build(),
            ),
            body,
        )
        .await
        .map_err(|_| denied("QA preview relay container could not be created"))?
        .id;
    docker
        .connect_network(browser_network, connect_request(&relay_id, "qa-preview"))
        .await
        .map_err(|_| denied("QA preview relay could not join the private browser network"))?;
    if *cancel.borrow() {
        return Err(denied("QA preview was cancelled before relay start"));
    }
    docker
        .start_container(&relay_id, None)
        .await
        .map_err(|_| denied("QA preview relay could not start"))?;
    wait_healthy(docker, &relay_id, readiness_timeout(spec.recipe), cancel).await?;
    Ok(relay_id)
}

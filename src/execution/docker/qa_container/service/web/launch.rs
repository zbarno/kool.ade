use bollard::Docker;

use super::super::super::{QaWebPreview, QaWebPreviewSpec, denied};
mod helpers;
mod task_service;
use super::{network_request, now_ms, remove_owned_network, validate_spec};
use crate::{
    execution::{
        docker::images::{ImageLock, RuntimeProfile},
        errors::ExecutionError,
    },
    runtime_qa::{QaPreviewRelayBindingV1, QaTaskServiceLeaseV1},
};
use helpers::remove_named;

pub async fn start_web_preview(
    docker: &Docker,
    spec: QaWebPreviewSpec<'_>,
    cancel: &mut tokio::sync::watch::Receiver<bool>,
) -> Result<QaWebPreview, ExecutionError> {
    let lock = ImageLock::bundled().map_err(|_| denied("QA image lock is invalid"))?;
    start_web_preview_for_lock(docker, spec, cancel, &lock).await
}

#[cfg(test)]
pub(crate) async fn start_web_preview_with_lock(
    docker: &Docker,
    spec: QaWebPreviewSpec<'_>,
    cancel: &mut tokio::sync::watch::Receiver<bool>,
    lock: &ImageLock,
) -> Result<QaWebPreview, ExecutionError> {
    start_web_preview_for_lock(docker, spec, cancel, lock).await
}

async fn start_web_preview_for_lock(
    docker: &Docker,
    spec: QaWebPreviewSpec<'_>,
    cancel: &mut tokio::sync::watch::Receiver<bool>,
    lock: &ImageLock,
) -> Result<QaWebPreview, ExecutionError> {
    validate_spec(&spec)?;
    if *cancel.borrow() {
        return Err(denied("QA web preview was cancelled before launch"));
    }
    let architecture = spec
        .browser_image
        .platform
        .strip_prefix("linux/")
        .unwrap_or_default();
    let required_tools = std::iter::once(spec.recipe.executable_id.clone())
        .chain(spec.recipe.prerequisite_tool_ids.iter().cloned())
        .collect::<Vec<_>>();
    if lock
        .select(RuntimeProfile::BrowserQa, architecture)
        .map_err(|_| denied("Browser QA image is unavailable"))?
        != *spec.browser_image
        || lock
            .select_with_tools(
                &[
                    RuntimeProfile::Node,
                    RuntimeProfile::Rust,
                    RuntimeProfile::Python,
                    RuntimeProfile::Go,
                    RuntimeProfile::General,
                    RuntimeProfile::BrowserQa,
                ],
                architecture,
                &required_tools,
            )
            .map_err(|_| denied("approved task runtime image is unavailable"))?
            != *spec.service_image
    {
        return Err(denied("QA web images differ from the reviewed image lock"));
    }
    let run = &spec.request.qa_run_id;
    let task_network_name = format!("koolade-qa-task-{run}");
    let browser_network_name = format!("koolade-qa-browser-{run}");
    let task_network = docker
        .create_network(network_request(
            task_network_name.clone(),
            spec.request,
            "task-network",
        ))
        .await
        .map_err(|_| denied("QA task network could not be created"))?
        .id;
    let browser_network = match docker
        .create_network(network_request(
            browser_network_name.clone(),
            spec.request,
            "browser-network",
        ))
        .await
    {
        Ok(response) => response.id,
        Err(_) => {
            remove_owned_network(docker, &task_network_name, spec.request, "task-network").await;
            return Err(denied("QA browser network could not be created"));
        }
    };
    let task_name = format!("koolade-qa-service-{run}");
    let relay_name = format!("koolade-qa-relay-{run}");
    let relay_token = format!("{:x}{:x}", uuid::Uuid::new_v4(), uuid::Uuid::new_v4());
    let result = task_service::launch_containers(
        docker,
        &spec,
        cancel,
        &task_network,
        &browser_network,
        &task_name,
        &relay_name,
        &relay_token,
    )
    .await;
    match result {
        Ok((task_id, relay_id)) => {
            let expiry = now_ms().saturating_add(
                (u64::from(spec.request.bounded_limits.timeout_seconds) + 120) * 1000,
            );
            let lease = QaTaskServiceLeaseV1 {
                schema_version: 1,
                qa_run_id: run.clone(),
                task_uid: spec.request.parent_task_uid.clone(),
                task_container_id: task_id.clone(),
                task_network_id: task_network.clone(),
                preview_port: spec.preview_port,
                task_build_sha: spec.request.build_artifact_sha256.clone(),
                approved_spec_hash: spec.request.approved_spec_hash.clone(),
                startup_recipe_ref: spec.recipe.recipe_id.clone(),
                startup_recipe_sha256: spec.request.startup_recipe_sha256.clone(),
                internal_network_only: true,
                published_ports: false,
                expires_at_unix_ms: expiry,
            };
            let binding = QaPreviewRelayBindingV1 {
                schema_version: 1,
                qa_run_id: run.clone(),
                task_uid: spec.request.parent_task_uid.clone(),
                task_execution_id: spec.request.execution_session_id.clone(),
                task_container_id: task_id,
                relay_container_id: relay_id,
                relay_token,
                browser_container_id: None,
                task_network_id: task_network,
                relay_browser_network_id: browser_network,
                target_preview_port: spec.preview_port,
                relay_port: 8080,
                preview_origin: "http://qa-preview:8080".into(),
                task_build_sha: spec.request.build_artifact_sha256.clone(),
                approved_spec_hash: spec.request.approved_spec_hash.clone(),
                startup_recipe_id: spec.recipe.recipe_id.clone(),
                startup_recipe_sha256: spec.request.startup_recipe_sha256.clone(),
                internal_networks_only: true,
                published_ports: false,
                recorded_at_unix_ms: now_ms(),
            };
            lease
                .validate_for(spec.request)
                .map_err(|_| denied("Created QA lease did not validate"))?;
            binding
                .validate_prelaunch()
                .map_err(|_| denied("Created QA relay binding did not validate"))?;
            Ok(QaWebPreview {
                service_lease: lease,
                relay_binding: binding,
            })
        }
        Err(error) => {
            remove_named(docker, &task_name, spec.request, "task-service").await;
            remove_named(docker, &relay_name, spec.request, "preview-relay").await;
            remove_owned_network(
                docker,
                &browser_network_name,
                spec.request,
                "browser-network",
            )
            .await;
            remove_owned_network(docker, &task_network_name, spec.request, "task-network").await;
            Err(error)
        }
    }
}

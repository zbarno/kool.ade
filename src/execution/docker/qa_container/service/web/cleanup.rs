use bollard::{Docker, query_parameters::RemoveContainerOptionsBuilder};

use super::super::super::denied;
use crate::{
    execution::errors::ExecutionError,
    runtime_qa::{QaPreviewRelayBindingV1, QaRunRequestV1, QaTaskServiceLeaseV1},
};

/// Remove only deterministic QA resources whose ownership labels still match
/// this immutable request. Called before retry after a process restart.
pub async fn recover_owned_web_preview(
    docker: &Docker,
    request: &QaRunRequestV1,
) -> Result<(), ExecutionError> {
    let run = &request.qa_run_id;
    remove_named_container(
        docker,
        &format!("koolade-qa-service-{run}"),
        request,
        "task-service",
    )
    .await?;
    remove_named_container(
        docker,
        &format!("koolade-qa-relay-{run}"),
        request,
        "preview-relay",
    )
    .await?;
    remove_named_network(
        docker,
        &format!("koolade-qa-browser-{run}"),
        request,
        "browser-network",
    )
    .await?;
    remove_named_network(
        docker,
        &format!("koolade-qa-task-{run}"),
        request,
        "task-network",
    )
    .await?;
    Ok(())
}

async fn remove_named_container(
    docker: &Docker,
    name: &str,
    request: &QaRunRequestV1,
    role: &str,
) -> Result<(), ExecutionError> {
    let Ok(inspect) = docker.inspect_container(name, None).await else {
        return Ok(());
    };
    let actual = inspect
        .config
        .and_then(|config| config.labels)
        .unwrap_or_default();
    if !matches_labels(&actual, request, role) {
        return Ok(());
    }
    docker
        .remove_container(
            name,
            Some(
                RemoveContainerOptionsBuilder::default()
                    .force(true)
                    .v(false)
                    .build(),
            ),
        )
        .await
        .map_err(|_| denied("owned orphan QA preview container could not be removed"))
}

async fn remove_named_network(
    docker: &Docker,
    name: &str,
    request: &QaRunRequestV1,
    role: &str,
) -> Result<(), ExecutionError> {
    let Ok(network) = docker.inspect_network(name, None).await else {
        return Ok(());
    };
    let actual = network.labels.unwrap_or_default();
    if !matches_labels(&actual, request, role) {
        return Ok(());
    }
    docker
        .remove_network(name)
        .await
        .map_err(|_| denied("owned orphan QA preview network could not be removed"))
}

fn matches_labels(
    actual: &std::collections::HashMap<String, String>,
    request: &QaRunRequestV1,
    role: &str,
) -> bool {
    let expected = super::labels(request, role);
    expected
        .iter()
        .all(|(key, value)| actual.get(key) == Some(value))
}

pub async fn cleanup_web_preview(
    docker: &Docker,
    lease: &QaTaskServiceLeaseV1,
    binding: &QaPreviewRelayBindingV1,
) -> Result<(), ExecutionError> {
    if lease.qa_run_id != binding.qa_run_id
        || lease.task_uid != binding.task_uid
        || lease.task_container_id != binding.task_container_id
        || lease.task_network_id != binding.task_network_id
    {
        return Err(denied("QA web preview cleanup identity mismatch"));
    }
    remove_container(
        docker,
        &lease.task_container_id,
        &lease.qa_run_id,
        &lease.task_uid,
        "task-service",
        &binding.task_execution_id,
    )
    .await?;
    remove_container(
        docker,
        &binding.relay_container_id,
        &lease.qa_run_id,
        &lease.task_uid,
        "preview-relay",
        &binding.task_execution_id,
    )
    .await?;
    remove_network(
        docker,
        &lease.task_network_id,
        &lease.qa_run_id,
        &lease.task_uid,
        "task-network",
        &binding.task_execution_id,
    )
    .await?;
    remove_network(
        docker,
        &binding.relay_browser_network_id,
        &lease.qa_run_id,
        &lease.task_uid,
        "browser-network",
        &binding.task_execution_id,
    )
    .await?;
    Ok(())
}

async fn remove_container(
    docker: &Docker,
    id: &str,
    run: &str,
    task: &str,
    role: &str,
    owner: &str,
) -> Result<(), ExecutionError> {
    let inspect = docker
        .inspect_container(id, None)
        .await
        .map_err(|_| denied("QA preview cleanup could not inspect its owned container"))?;
    let labels = inspect
        .config
        .and_then(|config| config.labels)
        .unwrap_or_default();
    if labels.get("com.koolade.qa.run").map(String::as_str) != Some(run)
        || labels.get("com.koolade.qa.task").map(String::as_str) != Some(task)
        || labels.get("com.koolade.qa.owner").map(String::as_str) != Some(owner)
        || labels.get("com.koolade.qa.role").map(String::as_str) != Some(role)
    {
        return Err(denied(
            "QA preview cleanup refused a container with mismatched ownership labels",
        ));
    }
    docker
        .remove_container(
            id,
            Some(
                RemoveContainerOptionsBuilder::default()
                    .force(true)
                    .v(false)
                    .build(),
            ),
        )
        .await
        .map_err(|_| denied("QA preview container cleanup could not be confirmed"))
}

async fn remove_network(
    docker: &Docker,
    id: &str,
    run: &str,
    task: &str,
    role: &str,
    owner: &str,
) -> Result<(), ExecutionError> {
    let inspect = docker
        .inspect_network(id, None)
        .await
        .map_err(|_| denied("QA preview cleanup could not inspect its owned network"))?;
    let labels = inspect.labels.unwrap_or_default();
    if labels.get("com.koolade.qa.run").map(String::as_str) != Some(run)
        || labels.get("com.koolade.qa.task").map(String::as_str) != Some(task)
        || labels.get("com.koolade.qa.owner").map(String::as_str) != Some(owner)
        || labels.get("com.koolade.qa.role").map(String::as_str) != Some(role)
    {
        return Err(denied(
            "QA preview cleanup refused a network with mismatched ownership labels",
        ));
    }
    docker
        .remove_network(id)
        .await
        .map_err(|_| denied("QA preview network cleanup could not be confirmed"))
}

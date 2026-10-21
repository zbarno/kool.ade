use bollard::models::{ContainerInspectResponse, NetworkInspect};

use super::super::denied;
use crate::{
    execution::errors::ExecutionError,
    runtime_qa::{QaPreviewRelayBindingV1, QaRunRequestV1, QaTaskServiceLeaseV1},
};

/// Inspect the app-leased service against Docker's current state before QA
/// access. Lease claims alone are not authority.
pub async fn validate_task_service_network(
    docker: &bollard::Docker,
    request: &QaRunRequestV1,
    lease: &QaTaskServiceLeaseV1,
) -> Result<(), ExecutionError> {
    lease
        .validate_for(request)
        .map_err(|_| denied("QA task service lease is invalid or expired"))?;
    let network = docker
        .inspect_network(&lease.task_network_id, None)
        .await
        .map_err(|_| denied("QA task service network cannot be inspected"))?;
    let container = docker
        .inspect_container(&lease.task_container_id, None)
        .await
        .map_err(|_| denied("QA task service container cannot be inspected"))?;
    validate_inspected_service(lease, &network, &container)
}

/// Validate the relay's two-network topology. The browser driver is connected
/// only to `relay_browser_network_id`; it never joins the task network.
pub async fn validate_preview_relay_network(
    docker: &bollard::Docker,
    request: &QaRunRequestV1,
    lease: &QaTaskServiceLeaseV1,
    binding: &QaPreviewRelayBindingV1,
) -> Result<(), ExecutionError> {
    validate_task_service_network(docker, request, lease).await?;
    binding
        .validate_prelaunch()
        .map_err(|_| denied("QA preview relay binding is invalid"))?;
    if binding.qa_run_id != request.qa_run_id
        || binding.task_uid != request.parent_task_uid
        || binding.task_execution_id != request.execution_session_id
        || binding.task_build_sha != request.build_artifact_sha256
        || binding.approved_spec_hash != request.approved_spec_hash
        || binding.startup_recipe_id != request.startup_recipe_ref
        || binding.startup_recipe_sha256 != request.startup_recipe_sha256
        || binding.task_container_id != lease.task_container_id
        || binding.task_network_id != lease.task_network_id
    {
        return Err(denied(
            "QA preview relay is bound to a different task or run",
        ));
    }
    let network = docker
        .inspect_network(&binding.relay_browser_network_id, None)
        .await
        .map_err(|_| denied("QA relay browser network cannot be inspected"))?;
    let relay = docker
        .inspect_container(&binding.relay_container_id, None)
        .await
        .map_err(|_| denied("QA preview relay container cannot be inspected"))?;
    validate_inspected_relay(binding, &network, &relay)
}

/// Verify the actual unstarted browser container ID before Docker starts it.
/// The browser may join only the relay network and may not publish host ports.
pub async fn validate_browser_driver_network(
    docker: &bollard::Docker,
    request: &QaRunRequestV1,
    binding: &QaPreviewRelayBindingV1,
    container_id: &str,
) -> Result<(), ExecutionError> {
    binding
        .validate()
        .map_err(|_| denied("QA preview relay binding is invalid"))?;
    if binding.browser_container_id.as_deref() != Some(container_id)
        || binding.qa_run_id != request.qa_run_id
        || binding.task_uid != request.parent_task_uid
    {
        return Err(denied("QA browser container identity is not durably bound"));
    }
    let container = docker
        .inspect_container(container_id, None)
        .await
        .map_err(|_| denied("QA browser container cannot be inspected"))?;
    let state = container.state.as_ref();
    let labels = container
        .config
        .as_ref()
        .and_then(|config| config.labels.as_ref());
    let networks = container
        .network_settings
        .as_ref()
        .and_then(|settings| settings.networks.as_ref());
    let host_ports = container
        .network_settings
        .as_ref()
        .and_then(|settings| settings.ports.as_ref())
        .is_some_and(|ports| {
            ports.values().any(|ports| {
                ports.as_ref().is_some_and(|ports| {
                    ports
                        .iter()
                        .any(|port| port.host_ip.is_some() || port.host_port.is_some())
                })
            })
        });
    if !state.is_some_and(|state| {
        state.running != Some(true)
            && state.status == Some(bollard::models::ContainerStateStatusEnum::CREATED)
    }) || labels.is_none_or(|labels| {
        labels.get(super::super::RUN_LABEL) != Some(&request.qa_run_id)
            || labels.get(super::super::TASK_LABEL) != Some(&request.parent_task_uid)
            || !matches!(
                labels.get(super::super::DRIVER_LABEL).map(String::as_str),
                Some("browser" | "service")
            )
            || labels.get(super::super::TASK_CONTAINER_LABEL) != Some(&binding.relay_container_id)
    }) || networks.is_none_or(|networks| {
        networks.len() != 1 || !networks.contains_key(&binding.relay_browser_network_id)
    }) || host_ports
    {
        return Err(denied(
            "QA browser container is running or outside its inspected private relay network",
        ));
    }
    Ok(())
}

fn validate_inspected_relay(
    binding: &QaPreviewRelayBindingV1,
    browser_network: &NetworkInspect,
    relay: &ContainerInspectResponse,
) -> Result<(), ExecutionError> {
    if browser_network.id.as_deref() != Some(binding.relay_browser_network_id.as_str())
        || browser_network.internal != Some(true)
        || browser_network.scope.as_deref() != Some("local")
        || !browser_network
            .containers
            .as_ref()
            .is_some_and(|containers| containers.contains_key(&binding.relay_container_id))
    {
        return Err(denied(
            "QA relay is not attached to its private browser network",
        ));
    }
    let state = relay.state.as_ref();
    let networks = relay
        .network_settings
        .as_ref()
        .and_then(|settings| settings.networks.as_ref());
    let ports = relay
        .network_settings
        .as_ref()
        .and_then(|settings| settings.ports.as_ref());
    let private_ports_only = ports.is_none_or(|ports| {
        ports.values().all(|bindings| {
            bindings.as_ref().is_none_or(|bindings| {
                bindings
                    .iter()
                    .all(|binding| binding.host_port.is_none() && binding.host_ip.is_none())
            })
        })
    });
    if !state.is_some_and(|state| {
        state.running == Some(true)
            && state.paused != Some(true)
            && state.health.as_ref().and_then(|health| health.status)
                == Some(bollard::models::HealthStatusEnum::HEALTHY)
    }) || networks.is_none_or(|networks| {
        networks.len() != 2
            || !networks.contains_key(&binding.task_network_id)
            || !networks.contains_key(&binding.relay_browser_network_id)
    }) || !private_ports_only
    {
        return Err(denied(
            "QA relay must be healthy on exactly two private networks with no published ports",
        ));
    }
    Ok(())
}

fn validate_inspected_service(
    lease: &QaTaskServiceLeaseV1,
    network: &NetworkInspect,
    container: &ContainerInspectResponse,
) -> Result<(), ExecutionError> {
    if network.id.as_deref() != Some(lease.task_network_id.as_str())
        || network.internal != Some(true)
        || network.scope.as_deref() != Some("local")
        || !network
            .containers
            .as_ref()
            .is_some_and(|containers| containers.contains_key(&lease.task_container_id))
    {
        return Err(denied(
            "QA task service is not on the leased internal network",
        ));
    }
    let state = container.state.as_ref();
    let settings = container.network_settings.as_ref();
    let bound_to_network = settings
        .and_then(|settings| settings.networks.as_ref())
        .is_some_and(|networks| networks.contains_key(&lease.task_network_id));
    let has_host_ports = settings
        .and_then(|settings| settings.ports.as_ref())
        .is_some_and(|ports| {
            ports.values().any(|bindings| {
                bindings.as_ref().is_some_and(|bindings| {
                    bindings
                        .iter()
                        .any(|binding| binding.host_port.is_some() || binding.host_ip.is_some())
                })
            })
        });
    if !state.is_some_and(|state| {
        state.running == Some(true)
            && state.paused != Some(true)
            && state.health.as_ref().and_then(|health| health.status)
                == Some(bollard::models::HealthStatusEnum::HEALTHY)
    }) || !bound_to_network
        || has_host_ports
    {
        return Err(denied(
            "QA task service must be healthy, attached to its internal network, and unpublished",
        ));
    }
    Ok(())
}

use crate::{
    execution::{docker::images::RuntimeProfile, errors::ExecutionError},
    runtime_qa::QaPlatform,
};

use super::{QaDriverContainerSpec, QaDriverNetwork, denied, driver_cache};

pub(super) fn validate(spec: &QaDriverContainerSpec<'_>) -> Result<(), ExecutionError> {
    spec.resources
        .validate()
        .map_err(|_| denied("QA resource budget is invalid"))?;
    driver_cache::validate_for_driver(
        spec.driver,
        spec.request,
        spec.dependency_caches,
        spec.image,
        spec.resources,
    )?;
    if spec.owner_id.is_empty()
        || spec.owner_id.len() > 128
        || spec.task_uid != spec.request.parent_task_uid
        || !super::valid_uuid(spec.owner_id)
        || !super::valid_uuid(spec.task_uid)
        || !driver_matches(
            spec.driver,
            spec.image.profile,
            &spec.request.relevant_platforms,
        )
        || spec.artifacts.is_empty()
        || spec.artifacts.len() > 2
    {
        return Err(denied(
            "QA driver container identity or artifacts are invalid",
        ));
    }

    match &spec.network {
        QaDriverNetwork::Disabled => {
            if spec.driver != "desktop" {
                return Err(denied(
                    "Web and headless service QA require a validated task relay",
                ));
            }
            if spec.service_lease.is_some() || spec.relay_binding.is_some() {
                return Err(denied(
                    "Linux desktop QA must not receive browser service or relay bindings",
                ));
            }
            Ok(())
        }
        QaDriverNetwork::InternalPreviewRelay {
            network_id,
            relay_container_id,
        } => validate_relay(spec, network_id, relay_container_id),
    }
}

fn driver_matches(driver: &str, profile: RuntimeProfile, platforms: &[QaPlatform]) -> bool {
    matches!(
        (driver, profile, platforms),
        ("browser", RuntimeProfile::BrowserQa, [QaPlatform::Web])
            | (
                "service",
                RuntimeProfile::BrowserQa,
                [QaPlatform::HeadlessService]
            )
            | (
                "desktop",
                RuntimeProfile::DesktopQa,
                [QaPlatform::LinuxDesktop]
            )
    )
}

fn validate_relay(
    spec: &QaDriverContainerSpec<'_>,
    network_id: &str,
    relay_container_id: &str,
) -> Result<(), ExecutionError> {
    let (Some(lease), Some(binding)) = (spec.service_lease, spec.relay_binding) else {
        return Err(denied(
            "Browser and headless service QA require a validated task relay binding",
        ));
    };
    lease
        .validate_for(spec.request)
        .map_err(|_| denied("QA service lease does not match the immutable run"))?;
    binding
        .validate_prelaunch()
        .map_err(|_| denied("QA preview relay binding is invalid"))?;
    if !relay_driver_matches(spec.driver, spec.image.profile)
        || !valid_container_id(network_id)
        || !valid_container_id(relay_container_id)
        || network_id != binding.relay_browser_network_id
        || relay_container_id != binding.relay_container_id
        || lease.task_network_id != binding.task_network_id
        || lease.task_container_id != binding.task_container_id
        || lease.preview_port != binding.target_preview_port
        || lease.startup_recipe_ref != binding.startup_recipe_id
        || lease.startup_recipe_sha256 != binding.startup_recipe_sha256
        || lease.qa_run_id != binding.qa_run_id
        || lease.task_uid != binding.task_uid
        || binding.qa_run_id != spec.request.qa_run_id
        || binding.task_uid != spec.request.parent_task_uid
        || binding.task_execution_id != spec.request.execution_session_id
        || binding.task_build_sha != spec.request.build_artifact_sha256
        || binding.approved_spec_hash != spec.request.approved_spec_hash
    {
        return Err(denied(
            "QA relay network is outside the exact task service and driver scope",
        ));
    }
    Ok(())
}

fn valid_container_id(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn relay_driver_matches(driver: &str, profile: RuntimeProfile) -> bool {
    matches!(driver, "browser" | "service") && profile == RuntimeProfile::BrowserQa
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn service_driver_is_bound_only_to_headless_service_on_browser_qa_image() {
        assert!(driver_matches(
            "service",
            RuntimeProfile::BrowserQa,
            &[QaPlatform::HeadlessService]
        ));
        assert!(!driver_matches(
            "service",
            RuntimeProfile::BrowserQa,
            &[QaPlatform::Web]
        ));
        assert!(!driver_matches(
            "service",
            RuntimeProfile::DesktopQa,
            &[QaPlatform::HeadlessService]
        ));
        assert!(!driver_matches(
            "browser",
            RuntimeProfile::BrowserQa,
            &[QaPlatform::HeadlessService]
        ));
    }

    #[test]
    fn task_relay_driver_allowlist_excludes_desktop_and_unknown_modes() {
        assert!(relay_driver_matches("browser", RuntimeProfile::BrowserQa));
        assert!(relay_driver_matches("service", RuntimeProfile::BrowserQa));
        assert!(!relay_driver_matches("desktop", RuntimeProfile::BrowserQa));
        assert!(!relay_driver_matches("service", RuntimeProfile::DesktopQa));
    }
}

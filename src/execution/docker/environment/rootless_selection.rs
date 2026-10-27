use super::{DockerEnvironmentState, DockerEnvironmentStatus};

pub(super) fn persist_rootless_selection(status: &mut DockerEnvironmentStatus) {
    if !cfg!(target_os = "linux") {
        return;
    }
    #[cfg(test)]
    if crate::execution::docker::engine::DockerEndpoint::
        test_support_endpoint_from_process_environment(
            crate::execution::docker::engine::HostPlatform::Linux,
        )
        .is_some()
    {
        return;
    }

    let selection_persisted = crate::persistence::linux_rootless_docker_selection::observe_current(
        status.endpoint_fingerprint.as_deref(),
        status.daemon_identity_fingerprint.as_deref(),
        status.rootless,
    )
    .unwrap_or(false);
    if status.local_execution_ready() && !selection_persisted {
        status.state = DockerEnvironmentState::CapabilitiesIncompatible;
        status.engine_ready = false;
        status.message = "Kool.ad/e could not confirm the current-user rootless socket identity in private local settings. Check socket ownership and application storage permissions, then recheck.".into();
    }
}

use super::{SetupScanResult, catalog};

pub(crate) fn verification_identity_matches(
    result: &SetupScanResult,
    selected: Option<catalog::ProviderIdV1>,
    endpoint_before: Option<String>,
    endpoint_after: Option<String>,
    docker: &crate::execution::docker::environment::DockerEnvironmentStatus,
) -> bool {
    let current_readiness_fingerprint = docker.readiness_fingerprint();
    selected == result.selected_provider
        && endpoint_before == endpoint_after
        && endpoint_after == result.endpoint_fingerprint
        && docker.endpoint_fingerprint == endpoint_after
        && docker.daemon_identity_fingerprint == result.daemon_identity_fingerprint
        && result.docker_readiness_fingerprint.as_deref()
            == Some(current_readiness_fingerprint.as_str())
}

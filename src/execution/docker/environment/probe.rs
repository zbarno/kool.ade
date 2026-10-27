use std::{path::Path, time::Duration};

use super::{DockerEnvironmentStatus, app_staging_root, process_endpoint};
use crate::execution::docker::{
    discovery::{PrerequisiteCode, PrerequisiteFailure},
    engine::{DockerEndpoint, EngineDiscovery, HostPlatform},
};

const PROBE_TIMEOUT: Duration = Duration::from_secs(8);
const DESKTOP_PROBE_TIMEOUT: Duration = Duration::from_secs(45);

pub(super) fn probe() -> DockerEnvironmentStatus {
    let platform = host_platform();
    let desktop_host = platform != HostPlatform::Linux;
    let endpoint = match process_endpoint(platform) {
        Ok(endpoint) => endpoint,
        Err(failure) => return DockerEnvironmentStatus::from_failure(failure, desktop_host),
    };
    let endpoint_fingerprint = endpoint_fingerprint(
        &endpoint,
        std::env::var("DOCKER_CONTEXT").ok().as_deref(),
        std::env::var("DOCKER_HOST").ok().as_deref(),
    );
    probe_endpoint(
        platform,
        endpoint,
        endpoint_fingerprint,
        EngineDiscovery::connect,
        app_staging_root,
    )
}

fn probe_endpoint(
    platform: HostPlatform,
    endpoint: DockerEndpoint,
    endpoint_fingerprint: String,
    connect_engine: impl FnOnce(DockerEndpoint) -> Result<EngineDiscovery, PrerequisiteFailure>,
    staging_root: impl FnOnce() -> Result<std::path::PathBuf, PrerequisiteFailure>,
) -> DockerEnvironmentStatus {
    let desktop_host = platform != HostPlatform::Linux;
    let desktop = endpoint.is_desktop();
    if platform != HostPlatform::Linux && !desktop {
        let mut status = DockerEnvironmentStatus::from_failure(
            PrerequisiteFailure {
                code: PrerequisiteCode::UntrustedEndpoint,
                message: "Local Docker execution requires a verified Docker Desktop endpoint on this platform".into(),
                retryable: false,
            },
            desktop_host,
        );
        status.endpoint_fingerprint = Some(endpoint_fingerprint);
        return status;
    }

    let engine = match connect_engine(endpoint.clone()) {
        Ok(engine) => engine,
        Err(failure) => {
            return failed_endpoint_status(failure, desktop_host, desktop, endpoint_fingerprint);
        }
    };
    let staging = match staging_root() {
        Ok(staging) => staging,
        Err(failure) => {
            return failed_endpoint_status(failure, desktop_host, desktop, endpoint_fingerprint);
        }
    };
    probe_engine(
        &engine,
        &endpoint,
        Path::new(&staging),
        endpoint_fingerprint,
    )
}

fn probe_engine(
    engine: &EngineDiscovery,
    endpoint: &DockerEndpoint,
    staging: &Path,
    endpoint_fingerprint: String,
) -> DockerEnvironmentStatus {
    let desktop_host = endpoint.is_desktop();
    let runtime = match tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime,
        Err(_) => {
            return failed_endpoint_status(
                PrerequisiteFailure {
                    code: PrerequisiteCode::CapabilitiesIncompatible,
                    message: "Could not create the Docker readiness probe runtime".into(),
                    retryable: false,
                },
                desktop_host,
                endpoint.is_desktop(),
                endpoint_fingerprint,
            );
        }
    };
    let timeout = if endpoint.is_desktop() {
        DESKTOP_PROBE_TIMEOUT
    } else {
        PROBE_TIMEOUT
    };
    let result =
        runtime.block_on(async { tokio::time::timeout(timeout, engine.probe(staging)).await });
    let mut status = match result {
        Ok(Ok(readiness)) => DockerEnvironmentStatus::from_readiness(readiness),
        Ok(Err(failure)) => DockerEnvironmentStatus::from_failure(failure, desktop_host),
        Err(_) => DockerEnvironmentStatus::from_failure(
            PrerequisiteFailure {
                code: PrerequisiteCode::DaemonStopped,
                message: "Docker Engine readiness probe timed out; start the daemon and retry"
                    .into(),
                retryable: true,
            },
            desktop_host,
        ),
    };
    status.desktop_vm |= endpoint.is_desktop();
    status.endpoint_fingerprint = Some(endpoint_fingerprint);
    status
}

fn failed_endpoint_status(
    failure: PrerequisiteFailure,
    desktop_host: bool,
    desktop_endpoint: bool,
    endpoint_fingerprint: String,
) -> DockerEnvironmentStatus {
    let mut status = DockerEnvironmentStatus::from_failure(failure, desktop_host);
    status.desktop_vm = desktop_endpoint;
    status.endpoint_fingerprint = Some(endpoint_fingerprint);
    status
}

pub fn run_smoke_test() -> Result<(), PrerequisiteFailure> {
    let platform = host_platform();
    if !smoke_host_supported(platform) {
        return Err(PrerequisiteFailure {
            code: PrerequisiteCode::UnsupportedOs,
            message: "Docker smoke testing is supported only on Linux, macOS, and Windows".into(),
            retryable: false,
        });
    }
    let endpoint = DockerEndpoint::from_process_environment(platform)?;
    if !smoke_endpoint_platform_shape(platform, &endpoint)
        || !endpoint.supports_application_execution()
    {
        return Err(PrerequisiteFailure {
            code: PrerequisiteCode::UntrustedEndpoint,
            message: "Docker smoke testing requires a verified native Linux rootless endpoint or verified local Docker Desktop endpoint".into(),
            retryable: false,
        });
    }
    let engine = EngineDiscovery::connect(endpoint)?;
    let staging = app_staging_root()?;
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|_| PrerequisiteFailure {
            code: PrerequisiteCode::CapabilitiesIncompatible,
            message: "Could not create the Docker smoke-test runtime".into(),
            retryable: false,
        })?;
    runtime.block_on(engine.test_environment(&staging))
}

fn smoke_host_supported(platform: HostPlatform) -> bool {
    match platform {
        HostPlatform::Linux => cfg!(target_os = "linux"),
        HostPlatform::MacOs => cfg!(target_os = "macos"),
        HostPlatform::Windows => cfg!(target_os = "windows"),
    }
}

fn smoke_endpoint_platform_shape(platform: HostPlatform, endpoint: &DockerEndpoint) -> bool {
    matches!(
        (platform, endpoint),
        (
            HostPlatform::Linux,
            DockerEndpoint::Unix { desktop: false, .. }
        ) | (
            HostPlatform::MacOs,
            DockerEndpoint::Unix { desktop: true, .. }
        ) | (
            HostPlatform::Windows,
            DockerEndpoint::NamedPipe { desktop: true, .. }
        )
    )
}

pub(super) fn endpoint_fingerprint(
    endpoint: &DockerEndpoint,
    docker_context: Option<&str>,
    docker_host: Option<&str>,
) -> String {
    use sha2::Digest as _;
    let mut digest = sha2::Sha256::new();
    digest.update(endpoint.fingerprint().as_bytes());
    digest.update([0]);
    for (key, value) in [
        ("DOCKER_CONTEXT", docker_context),
        ("DOCKER_HOST", docker_host),
    ] {
        digest.update(key.as_bytes());
        digest.update([0]);
        if let Some(value) = value {
            digest.update(value.as_bytes());
        }
        digest.update([0]);
    }
    format!("{:x}", digest.finalize())
}

pub(super) fn host_platform() -> HostPlatform {
    if cfg!(target_os = "macos") {
        HostPlatform::MacOs
    } else if cfg!(target_os = "windows") {
        HostPlatform::Windows
    } else {
        HostPlatform::Linux
    }
}

#[cfg(test)]
#[path = "probe/tests.rs"]
mod tests;

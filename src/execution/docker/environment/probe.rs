use std::{path::Path, time::Duration};

use super::{DockerEnvironmentStatus, app_staging_root};
use crate::execution::docker::{
    discovery::{PrerequisiteCode, PrerequisiteFailure},
    engine::{DockerEndpoint, EngineDiscovery, HostPlatform},
};

const PROBE_TIMEOUT: Duration = Duration::from_secs(8);

pub(super) fn probe() -> DockerEnvironmentStatus {
    let platform = host_platform();
    let desktop_host = platform != HostPlatform::Linux;
    let endpoint = match DockerEndpoint::from_process_environment(platform) {
        Ok(endpoint) => endpoint,
        Err(failure) => return DockerEnvironmentStatus::from_failure(failure, desktop_host),
    };
    let desktop = endpoint.is_desktop();
    let endpoint_fingerprint = endpoint_fingerprint(
        &endpoint,
        std::env::var("DOCKER_CONTEXT").ok().as_deref(),
        std::env::var("DOCKER_HOST").ok().as_deref(),
    );
    if platform != HostPlatform::Linux {
        let failure = if !desktop {
            PrerequisiteFailure {
                code: PrerequisiteCode::UntrustedEndpoint,
                message: "Local Docker execution is enabled only for verified native Linux rootless Engine".into(),
                retryable: false,
            }
        } else {
            match &endpoint {
                DockerEndpoint::Unix { path, .. } => match std::fs::symlink_metadata(path) {
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                        PrerequisiteFailure {
                            code: PrerequisiteCode::DockerMissing,
                            message: "Docker Desktop's local Linux endpoint is not present".into(),
                            retryable: true,
                        }
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::PermissionDenied => {
                        PrerequisiteFailure {
                            code: PrerequisiteCode::PermissionDenied,
                            message:
                                "Permission denied inspecting the local Docker Desktop endpoint"
                                    .into(),
                            retryable: false,
                        }
                    }
                    Ok(metadata) if endpoint_is_socket(&metadata) => PrerequisiteFailure {
                        code: PrerequisiteCode::UntrustedEndpoint,
                        message:
                            "Docker Desktop transport identity is not verified on this platform"
                                .into(),
                        retryable: false,
                    },
                    _ => PrerequisiteFailure {
                        code: PrerequisiteCode::UntrustedEndpoint,
                        message: "Docker Desktop endpoint is not a verified local Linux socket"
                            .into(),
                        retryable: false,
                    },
                },
                DockerEndpoint::NamedPipe { .. } => PrerequisiteFailure {
                    code: PrerequisiteCode::UntrustedEndpoint,
                    message: "Docker Desktop named-pipe identity is not verified on this platform"
                        .into(),
                    retryable: false,
                },
            }
        };
        let mut status = DockerEnvironmentStatus::from_failure(failure, desktop);
        status.endpoint_fingerprint = Some(endpoint_fingerprint);
        status.desktop_vm = desktop;
        return status;
    }

    let engine = match EngineDiscovery::connect(endpoint) {
        Ok(engine) => engine,
        Err(failure) => {
            let mut status = DockerEnvironmentStatus::from_failure(failure, desktop);
            status.endpoint_fingerprint = Some(endpoint_fingerprint);
            return status;
        }
    };
    let Ok(runtime) = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    else {
        let mut status = DockerEnvironmentStatus::from_failure(
            PrerequisiteFailure {
                code: PrerequisiteCode::CapabilitiesIncompatible,
                message: "Could not create the Docker readiness probe runtime".into(),
                retryable: false,
            },
            desktop,
        );
        status.endpoint_fingerprint = Some(endpoint_fingerprint);
        return status;
    };
    let staging = match app_staging_root() {
        Ok(staging) => staging,
        Err(failure) => {
            let mut status = DockerEnvironmentStatus::from_failure(failure, desktop);
            status.endpoint_fingerprint = Some(endpoint_fingerprint);
            return status;
        }
    };
    let result = runtime.block_on(async {
        tokio::time::timeout(PROBE_TIMEOUT, engine.probe(Path::new(&staging))).await
    });
    let mut status = match result {
        Ok(Ok(readiness)) => DockerEnvironmentStatus::from_readiness(readiness),
        Ok(Err(failure)) => DockerEnvironmentStatus::from_failure(failure, desktop),
        Err(_) => DockerEnvironmentStatus::from_failure(
            PrerequisiteFailure {
                code: PrerequisiteCode::DaemonStopped,
                message: "Docker Engine readiness probe timed out; start the daemon and retry"
                    .into(),
                retryable: true,
            },
            desktop,
        ),
    };
    status.endpoint_fingerprint = Some(endpoint_fingerprint);
    status
}

pub fn run_smoke_test() -> Result<(), PrerequisiteFailure> {
    let platform = host_platform();
    if platform != HostPlatform::Linux {
        return Err(PrerequisiteFailure {
            code: PrerequisiteCode::UnsupportedOs,
            message:
                "Local Docker execution is enabled only for verified native Linux rootless Engine"
                    .into(),
            retryable: false,
        });
    }
    let endpoint = DockerEndpoint::from_process_environment(platform)?;
    if !endpoint.supports_application_execution() {
        return Err(PrerequisiteFailure {
            code: PrerequisiteCode::UntrustedEndpoint,
            message: "Local Docker execution requires a verified native Linux endpoint".into(),
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

#[cfg(unix)]
fn endpoint_is_socket(metadata: &std::fs::Metadata) -> bool {
    use std::os::unix::fs::FileTypeExt;
    metadata.file_type().is_socket()
}

#[cfg(not(unix))]
fn endpoint_is_socket(_metadata: &std::fs::Metadata) -> bool {
    false
}

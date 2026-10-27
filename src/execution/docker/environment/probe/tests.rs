use super::{
    host_platform, probe_endpoint, probe_engine, smoke_endpoint_platform_shape,
    smoke_host_supported,
};
use crate::execution::docker::environment::DockerEnvironmentState;
use crate::execution::docker::{
    discovery::{PrerequisiteCode, PrerequisiteFailure},
    engine::{
        DockerApi, DockerEndpoint, EngineDiscovery, EngineFuture, EngineSnapshot, HostPlatform,
    },
    images::{ImageObservation, SelectedImage},
};
use std::sync::Arc;

struct DesktopApi(EngineSnapshot);

impl DockerApi for DesktopApi {
    fn inspect<'a>(&'a self) -> EngineFuture<'a, Result<EngineSnapshot, PrerequisiteFailure>> {
        let snapshot = self.0.clone();
        Box::pin(async move { Ok(snapshot) })
    }

    fn image_digests<'a>(&'a self) -> EngineFuture<'a, Result<Vec<String>, PrerequisiteFailure>> {
        Box::pin(async { Ok(Vec::new()) })
    }

    fn ensure_image<'a>(
        &'a self,
        _image: &'a SelectedImage,
        _expected_engine_fingerprint: Option<&'a str>,
    ) -> EngineFuture<'a, Result<ImageObservation, PrerequisiteFailure>> {
        Box::pin(async {
            Err(PrerequisiteFailure {
                code: PrerequisiteCode::ImageUnavailable,
                message: "synthetic Desktop probe does not pull images".into(),
                retryable: false,
            })
        })
    }
}

fn desktop_snapshot() -> EngineSnapshot {
    EngineSnapshot {
        daemon_id: "synthetic-daemon".into(),
        daemon_version: "28.0.0".into(),
        api_version: "1.53".into(),
        minimum_api_version: "1.41".into(),
        os_type: "linux".into(),
        architecture: "amd64".into(),
        security_options: vec!["name=seccomp,profile=builtin".into()],
        rootless: false,
        native_linux: false,
        docker_desktop: true,
        memory_limit: true,
        cpu_quota: true,
        pids_limit: true,
        cgroup_version: Some("2".into()),
        cgroup_driver: Some("systemd".into()),
        seccomp_enabled: true,
        docker_storage_accessible: false,
        disk_available_bytes: None,
        minimum_disk_bytes: 1,
    }
}

fn engine(snapshot: EngineSnapshot, verified_transport: bool) -> EngineDiscovery {
    let endpoint = DockerEndpoint::NamedPipe {
        path: r"\\.\pipe\docker_engine".into(),
        desktop: true,
    };
    engine_for(endpoint, snapshot, verified_transport)
}

fn engine_for(
    endpoint: DockerEndpoint,
    snapshot: EngineSnapshot,
    verified_transport: bool,
) -> EngineDiscovery {
    let api = Arc::new(DesktopApi(snapshot));
    if verified_transport {
        EngineDiscovery::with_test_verified_desktop_transport(endpoint, api)
    } else {
        EngineDiscovery::with_api(endpoint, api)
    }
}

#[test]
fn macos_desktop_probe_reaches_engine_discovery_through_the_desktop_branch() {
    let workspace = tempfile::tempdir().unwrap();
    let endpoint = DockerEndpoint::Unix {
        path: "/Users/example/.docker/run/docker.sock".into(),
        desktop: true,
    };
    let mut engine_called = false;
    let result = probe_endpoint(
        HostPlatform::MacOs,
        endpoint.clone(),
        "d".repeat(64),
        |connected_endpoint| {
            engine_called = true;
            assert_eq!(connected_endpoint, endpoint);
            Ok(engine_for(connected_endpoint, desktop_snapshot(), true))
        },
        || Ok(workspace.path().to_path_buf()),
    );

    assert!(engine_called);
    assert!(result.desktop_transport_verified);
    assert_eq!(result.state, DockerEnvironmentState::InsufficientDisk);
}

#[test]
fn windows_desktop_probe_reaches_engine_discovery_through_the_pipe_branch() {
    let workspace = tempfile::tempdir().unwrap();
    let endpoint = DockerEndpoint::NamedPipe {
        path: r"\\.\pipe\docker_engine".into(),
        desktop: true,
    };
    let mut engine_called = false;
    let result = probe_endpoint(
        HostPlatform::Windows,
        endpoint.clone(),
        "e".repeat(64),
        |connected_endpoint| {
            engine_called = true;
            assert_eq!(connected_endpoint, endpoint);
            Ok(engine_for(connected_endpoint, desktop_snapshot(), true))
        },
        || Ok(workspace.path().to_path_buf()),
    );

    assert!(engine_called);
    assert!(result.desktop_transport_verified);
    assert_eq!(result.state, DockerEnvironmentState::InsufficientDisk);
}

#[test]
fn verified_desktop_transport_is_separate_from_missing_vm_storage() {
    let workspace = tempfile::tempdir().unwrap();
    let endpoint = DockerEndpoint::NamedPipe {
        path: r"\\.\pipe\docker_engine".into(),
        desktop: true,
    };
    let result = probe_engine(
        &engine(desktop_snapshot(), true),
        &endpoint,
        workspace.path(),
        "a".repeat(64),
    );

    assert!(result.desktop_vm);
    assert!(result.desktop_transport_verified);
    assert!(!result.engine_ready);
    assert_eq!(result.state, DockerEnvironmentState::InsufficientDisk);
    assert!(!result.local_execution_ready());
}

#[test]
fn a_desktop_path_and_healthy_snapshot_do_not_create_transport_attestation() {
    let workspace = tempfile::tempdir().unwrap();
    let endpoint = DockerEndpoint::NamedPipe {
        path: r"\\.\pipe\docker_engine".into(),
        desktop: true,
    };
    let mut snapshot = desktop_snapshot();
    snapshot.docker_storage_accessible = true;
    snapshot.disk_available_bytes = Some(1024);
    let result = probe_engine(
        &engine(snapshot, false),
        &endpoint,
        workspace.path(),
        "b".repeat(64),
    );

    assert!(!result.engine_ready);
    assert!(result.desktop_vm);
    assert!(!result.desktop_transport_verified);
    assert_eq!(result.state, DockerEnvironmentState::InsufficientDisk);
}

#[test]
fn desktop_transport_attestation_requires_linux_container_engine_checks() {
    let workspace = tempfile::tempdir().unwrap();
    let endpoint = DockerEndpoint::NamedPipe {
        path: r"\\.\pipe\docker_engine".into(),
        desktop: true,
    };
    let mut snapshot = desktop_snapshot();
    snapshot.os_type = "windows".into();
    snapshot.docker_storage_accessible = true;
    snapshot.disk_available_bytes = Some(1024);
    let result = probe_engine(
        &engine(snapshot, true),
        &endpoint,
        workspace.path(),
        "c".repeat(64),
    );

    assert!(result.desktop_vm);
    assert!(!result.desktop_transport_verified);
    assert!(!result.engine_ready);
    assert_eq!(
        result.state,
        DockerEnvironmentState::WindowsContainersRequired
    );
}

#[test]
fn smoke_wrapper_accepts_only_native_endpoint_families() {
    let macos_desktop = DockerEndpoint::Unix {
        path: "/Users/example/.docker/run/docker.sock".into(),
        desktop: true,
    };
    let windows_desktop = DockerEndpoint::NamedPipe {
        path: r"\\.\pipe\docker_engine".into(),
        desktop: true,
    };
    let linux_rootless = DockerEndpoint::Unix {
        path: "/run/user/1000/docker.sock".into(),
        desktop: false,
    };
    let untrusted_pipe = DockerEndpoint::NamedPipe {
        path: r"\\.\pipe\docker_engine".into(),
        desktop: false,
    };

    assert!(smoke_endpoint_platform_shape(
        HostPlatform::MacOs,
        &macos_desktop
    ));
    assert!(smoke_endpoint_platform_shape(
        HostPlatform::Windows,
        &windows_desktop
    ));
    assert!(smoke_endpoint_platform_shape(
        HostPlatform::Linux,
        &linux_rootless
    ));
    assert!(!smoke_endpoint_platform_shape(
        HostPlatform::MacOs,
        &windows_desktop
    ));
    assert!(!smoke_endpoint_platform_shape(
        HostPlatform::Windows,
        &macos_desktop
    ));
    assert!(!smoke_endpoint_platform_shape(
        HostPlatform::Linux,
        &macos_desktop
    ));
    assert!(!smoke_endpoint_platform_shape(
        HostPlatform::Windows,
        &untrusted_pipe
    ));
    assert!(smoke_host_supported(host_platform()));
}

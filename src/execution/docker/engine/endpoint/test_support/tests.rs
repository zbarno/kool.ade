#[cfg(target_os = "linux")]
use super::super::{DockerEndpoint, HostPlatform};
#[cfg(unix)]
use super::{daemon_restart_endpoint_from_environment, rootless_action_socket_for};

#[cfg(unix)]
#[test]
fn rootless_action_socket_requires_explicit_owned_action_socket() {
    use std::{os::unix::net::UnixListener, path::Path};

    let temp = tempfile::tempdir().unwrap();
    let home = std::fs::canonicalize(temp.path()).unwrap();
    let action_dir = home.join("setup-docker-action");
    let run_dir = action_dir.join("run-test");
    std::fs::create_dir_all(&run_dir).unwrap();
    let socket_path = run_dir.join("docker.sock");
    let _listener = UnixListener::bind(&socket_path).unwrap();
    assert!(rootless_action_socket_for(
        &socket_path,
        &socket_path,
        &home
    ));
    assert!(!rootless_action_socket_for(
        &socket_path,
        Path::new("/tmp/docker.sock"),
        &home
    ));
    assert!(!rootless_action_socket_for(
        Path::new("/tmp/docker.sock"),
        Path::new("/tmp/docker.sock"),
        &home
    ));
}

#[cfg(target_os = "linux")]
#[test]
fn daemon_restart_probe_endpoint_is_test_only_and_requires_opt_in_without_context() {
    let socket = std::env::temp_dir().join(format!(
        "koolade-docker-{}.sock",
        uuid::Uuid::new_v4().simple()
    ));
    let host = format!("unix://{}", socket.display());
    let mut environment = crate::test_support::ProcessEnvironmentGuard::new();
    environment.set("DOCKER_HOST", &host);
    environment.remove("DOCKER_CONTEXT");
    environment.remove("KOOLADE_DOCKER_DAEMON_RESTART_INTEGRATION");

    assert!(daemon_restart_endpoint_from_environment().is_none());
    assert!(DockerEndpoint::from_process_environment(HostPlatform::Linux).is_err());

    environment.set("KOOLADE_DOCKER_DAEMON_RESTART_INTEGRATION", "1");
    assert_eq!(
        daemon_restart_endpoint_from_environment(),
        Some(DockerEndpoint::Unix {
            path: socket.clone(),
            desktop: false,
        })
    );
    assert!(DockerEndpoint::from_process_environment(HostPlatform::Linux).is_err());
    assert_eq!(
        DockerEndpoint::from_process_environment_with_test_support(HostPlatform::Linux),
        Ok(DockerEndpoint::Unix {
            path: socket,
            desktop: false,
        })
    );

    environment.set("DOCKER_CONTEXT", "default");
    assert!(daemon_restart_endpoint_from_environment().is_none());
    assert!(
        DockerEndpoint::from_process_environment_with_test_support(HostPlatform::Linux).is_err()
    );
}

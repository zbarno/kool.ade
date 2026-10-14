use super::super::discovery::PrerequisiteCode;
use super::super::engine::{DockerEndpoint, HostPlatform};

#[test]
fn endpoint_rejects_tcp_http_ssh_and_unverified_contexts() {
    for endpoint in [
        "tcp://127.0.0.1:2375",
        "http://localhost:2375",
        "ssh://builder",
    ] {
        assert_eq!(
            DockerEndpoint::parse_local_host(endpoint, None)
                .unwrap_err()
                .code,
            PrerequisiteCode::UntrustedEndpoint
        );
    }
    let context = DockerEndpoint::from_environment(
        None,
        Some("production"),
        None,
        None,
        None,
        HostPlatform::Linux,
    )
    .unwrap_err();
    assert_eq!(context.code, PrerequisiteCode::UntrustedEndpoint);
    let selected = DockerEndpoint::from_environment(
        Some("tcp://127.0.0.1:2375"),
        Some("production"),
        None,
        None,
        None,
        HostPlatform::Linux,
    )
    .unwrap_err();
    assert_eq!(selected.code, PrerequisiteCode::UntrustedEndpoint);
}

#[test]
fn endpoint_rejects_arbitrary_unix_sockets() {
    for endpoint in [
        "unix:///tmp/docker-proxy.sock",
        "unix:///tmp/user/1234/docker.sock",
    ] {
        assert_eq!(
            DockerEndpoint::parse_local_host(endpoint, None)
                .unwrap_err()
                .code,
            PrerequisiteCode::UntrustedEndpoint,
            "{endpoint}"
        );
    }
}

#[test]
fn accepts_only_local_transport_addresses() {
    #[cfg(target_os = "linux")]
    {
        let uid = unsafe { libc::geteuid() };
        let endpoint =
            DockerEndpoint::parse_local_host(&format!("unix:///run/user/{uid}/docker.sock"), None)
                .unwrap();
        assert_eq!(
            endpoint,
            DockerEndpoint::Unix {
                path: format!("/run/user/{uid}/docker.sock").into(),
                desktop: false
            }
        );
        let other_uid = uid.wrapping_add(1);
        assert_eq!(
            DockerEndpoint::parse_local_host(
                &format!("unix:///run/user/{other_uid}/docker.sock"),
                None,
            )
            .unwrap_err()
            .code,
            PrerequisiteCode::UntrustedEndpoint
        );
    }
    let named_pipe =
        DockerEndpoint::parse_local_host("npipe:////./pipe/docker_engine", None).unwrap();
    assert!(named_pipe.is_desktop());
}

#[test]
#[cfg(target_os = "linux")]
fn untrusted_xdg_runtime_directory_cannot_redirect_docker_discovery() {
    let endpoint = DockerEndpoint::from_environment(
        None,
        None,
        Some(std::path::Path::new("/tmp/untrusted-runtime")),
        None,
        Some(1000),
        HostPlatform::Linux,
    )
    .unwrap();
    assert!(matches!(
        endpoint,
        DockerEndpoint::Unix { path, desktop: false }
            if path == std::path::Path::new("/run/user/1000/docker.sock")
                || path == std::path::Path::new("/var/run/docker.sock")
    ));
}

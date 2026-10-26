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

#[cfg(target_os = "linux")]
#[test]
fn isolated_daemon_socket_requires_the_opted_in_test_fixture() {
    let mut environment = crate::test_support::ProcessEnvironmentGuard::new();
    environment.remove("KOOLADE_DOCKER_DAEMON_RESTART_INTEGRATION");
    let socket = std::env::temp_dir().join(format!(
        "koolade-docker-{}.sock",
        uuid::Uuid::new_v4().simple()
    ));
    let host = format!("unix://{}", socket.display());
    assert_eq!(
        DockerEndpoint::from_environment(
            Some(&host),
            None,
            None,
            None,
            Some(unsafe { libc::geteuid() }),
            HostPlatform::Linux,
        )
        .unwrap_err()
        .code,
        PrerequisiteCode::UntrustedEndpoint
    );

    environment.set("KOOLADE_DOCKER_DAEMON_RESTART_INTEGRATION", "1");
    assert_eq!(
        DockerEndpoint::from_environment(
            Some(&host),
            None,
            None,
            None,
            Some(unsafe { libc::geteuid() }),
            HostPlatform::Linux,
        )
        .unwrap(),
        DockerEndpoint::Unix {
            path: socket,
            desktop: false,
        }
    );
}

#[test]
fn endpoint_fingerprint_is_redacted_and_changes_with_local_endpoint_identity() {
    let rootful = DockerEndpoint::from_environment(
        Some("unix:///var/run/docker.sock"),
        None,
        None,
        None,
        None,
        HostPlatform::Linux,
    )
    .unwrap();
    let desktop = DockerEndpoint::from_environment(
        None,
        Some("desktop-linux"),
        None,
        Some(std::path::Path::new("/home/synthetic")),
        None,
        HostPlatform::Linux,
    )
    .unwrap();

    let first = rootful.fingerprint();
    let second = desktop.fingerprint();

    assert_ne!(first, second);
    assert!(!first.contains("/var/run"));
    assert!(!second.contains("/home/synthetic"));
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
fn macos_default_and_desktop_context_select_the_local_desktop_socket() {
    let home = std::path::Path::new("/Users/example");
    let expected = DockerEndpoint::Unix {
        path: home.join(".docker/run/docker.sock"),
        desktop: true,
    };

    assert_eq!(
        DockerEndpoint::from_environment(None, None, None, Some(home), None, HostPlatform::MacOs)
            .unwrap(),
        expected
    );
    assert_eq!(
        DockerEndpoint::from_environment(
            None,
            Some("desktop-linux"),
            None,
            Some(home),
            None,
            HostPlatform::MacOs,
        )
        .unwrap(),
        expected
    );
}

#[test]
fn windows_default_and_desktop_context_select_the_local_named_pipe() {
    let expected = DockerEndpoint::NamedPipe {
        path: r"\\.\pipe\docker_engine".into(),
        desktop: true,
    };

    assert_eq!(
        DockerEndpoint::from_environment(None, None, None, None, None, HostPlatform::Windows)
            .unwrap(),
        expected
    );
    assert_eq!(
        DockerEndpoint::from_environment(
            None,
            Some("desktop-linux"),
            None,
            None,
            None,
            HostPlatform::Windows,
        )
        .unwrap(),
        expected
    );
}

#[test]
fn desktop_endpoints_never_authorize_application_owned_execution() {
    let mac = DockerEndpoint::Unix {
        path: "/synthetic/docker.sock".into(),
        desktop: true,
    };
    let windows = DockerEndpoint::NamedPipe {
        path: r"\\.\pipe\docker_engine".into(),
        desktop: true,
    };
    assert!(!mac.supports_application_execution());
    assert!(!windows.supports_application_execution());
    #[cfg(target_os = "linux")]
    assert!(
        DockerEndpoint::Unix {
            path: "/var/run/docker.sock".into(),
            desktop: false,
        }
        .supports_application_execution()
    );
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

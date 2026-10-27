use super::super::discovery::PrerequisiteCode;
use super::super::engine::{DockerEndpoint, HostPlatform};

#[cfg(target_os = "linux")]
fn linux_endpoint(
    docker_host: Option<&str>,
    docker_context: Option<&str>,
    runtime_dir: Option<&std::path::Path>,
) -> Result<DockerEndpoint, super::super::discovery::PrerequisiteFailure> {
    DockerEndpoint::from_environment(
        docker_host,
        docker_context,
        runtime_dir,
        None,
        Some(linux_test_uid()),
        HostPlatform::Linux,
    )
}

#[cfg(target_os = "linux")]
fn linux_test_uid() -> u32 {
    let uid = unsafe { libc::geteuid() };
    if uid == 0 { 12345 } else { uid }
}

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
fn isolated_daemon_socket_opt_in_cannot_override_the_current_user_endpoint() {
    let mut environment = crate::test_support::ProcessEnvironmentGuard::new();
    environment.remove("KOOLADE_DOCKER_DAEMON_RESTART_INTEGRATION");
    let socket = std::env::temp_dir().join(format!(
        "koolade-docker-{}.sock",
        uuid::Uuid::new_v4().simple()
    ));
    let host = format!("unix://{}", socket.display());
    let reject_untrusted_endpoint = || {
        DockerEndpoint::from_environment(
            Some(&host),
            None,
            None,
            None,
            Some(linux_test_uid()),
            HostPlatform::Linux,
        )
        .unwrap_err()
        .code
    };
    assert_eq!(
        reject_untrusted_endpoint(),
        PrerequisiteCode::UntrustedEndpoint
    );

    environment.set("KOOLADE_DOCKER_DAEMON_RESTART_INTEGRATION", "1");
    assert_eq!(
        reject_untrusted_endpoint(),
        PrerequisiteCode::UntrustedEndpoint
    );
}

#[test]
fn endpoint_fingerprint_is_redacted_and_changes_with_local_endpoint_identity() {
    #[cfg(target_os = "linux")]
    let uid = linux_test_uid();
    #[cfg(not(target_os = "linux"))]
    let uid = 12345;
    let rootless_host = format!("unix:///run/user/{uid}/docker.sock");
    let rootless = DockerEndpoint::from_environment(
        Some(&rootless_host),
        None,
        None,
        None,
        Some(uid),
        HostPlatform::Linux,
    )
    .unwrap();
    let desktop = DockerEndpoint::from_environment(
        None,
        Some("desktop-linux"),
        None,
        Some(std::path::Path::new("/Users/synthetic")),
        None,
        HostPlatform::MacOs,
    )
    .unwrap();

    let first = rootless.fingerprint();
    let second = desktop.fingerprint();

    assert_ne!(first, second);
    assert!(!first.contains(&format!("/run/user/{uid}")));
    assert!(!second.contains("/Users/synthetic"));
}

#[test]
#[cfg(target_os = "linux")]
fn clean_desktop_environment_resolves_the_current_users_rootless_socket() {
    let uid = linux_test_uid();
    let expected = DockerEndpoint::Unix {
        path: format!("/run/user/{uid}/docker.sock").into(),
        desktop: false,
    };
    for runtime_dir in [None, Some(std::path::Path::new("/tmp/untrusted-runtime"))] {
        assert_eq!(linux_endpoint(None, None, runtime_dir).unwrap(), expected);
    }
}

#[test]
#[cfg(target_os = "linux")]
fn exact_current_user_docker_host_is_accepted() {
    let uid = linux_test_uid();
    let host = format!("unix:///run/user/{uid}/docker.sock");
    assert_eq!(
        linux_endpoint(Some(&host), None, None).unwrap(),
        DockerEndpoint::Unix {
            path: format!("/run/user/{uid}/docker.sock").into(),
            desktop: false,
        }
    );
}

#[test]
#[cfg(target_os = "linux")]
fn linux_docker_context_overrides_fail_with_actionable_endpoint_errors() {
    let uid = linux_test_uid();
    let host = format!("unix:///run/user/{uid}/docker.sock");
    for context in ["default", "desktop-linux", "production"] {
        let error = linux_endpoint(Some(&host), Some(context), None).unwrap_err();
        assert_eq!(error.code, PrerequisiteCode::UntrustedEndpoint, "{context}");
        assert!(
            error.message.contains("DOCKER_CONTEXT"),
            "{context}: {}",
            error.message
        );
        assert!(
            error.message.contains("clear"),
            "{context}: {}",
            error.message
        );
    }
}

#[test]
#[cfg(target_os = "linux")]
fn linux_docker_host_must_be_the_exact_current_user_socket() {
    let uid = linux_test_uid();
    let wrong_uid = uid.wrapping_add(1);
    for host in [
        "unix:///var/run/docker.sock".to_owned(),
        "unix:///tmp/docker.sock".to_owned(),
        "tcp://127.0.0.1:2375".to_owned(),
        format!("unix:///run/user/{wrong_uid}/docker.sock"),
    ] {
        let error = linux_endpoint(Some(&host), None, None).unwrap_err();
        assert_eq!(error.code, PrerequisiteCode::UntrustedEndpoint, "{host}");
        assert!(
            error.message.contains("DOCKER_HOST"),
            "{host}: {}",
            error.message
        );
    }
    let context_conflict =
        linux_endpoint(Some("unix:///var/run/docker.sock"), Some("rootless"), None).unwrap_err();
    assert_eq!(context_conflict.code, PrerequisiteCode::UntrustedEndpoint);
    assert!(context_conflict.message.contains("DOCKER_HOST"));
}

#[test]
#[cfg(target_os = "linux")]
fn root_user_cannot_select_a_rootless_docker_endpoint() {
    let error =
        DockerEndpoint::from_environment(None, None, None, None, Some(0), HostPlatform::Linux)
            .unwrap_err();
    assert_eq!(error.code, PrerequisiteCode::UntrustedEndpoint);
}

#[test]
fn accepts_only_local_transport_addresses() {
    #[cfg(target_os = "linux")]
    {
        let uid = linux_test_uid();
        let endpoint = DockerEndpoint::from_environment(
            Some(&format!("unix:///run/user/{uid}/docker.sock")),
            None,
            None,
            None,
            Some(uid),
            HostPlatform::Linux,
        )
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
            DockerEndpoint::from_environment(
                Some(&format!("unix:///run/user/{other_uid}/docker.sock")),
                None,
                None,
                None,
                Some(uid),
                HostPlatform::Linux,
            )
            .unwrap_err()
            .code,
            PrerequisiteCode::UntrustedEndpoint
        );
    }
    let named_pipe = DockerEndpoint::from_environment(
        Some("npipe:////./pipe/docker_engine"),
        None,
        None,
        None,
        None,
        HostPlatform::Windows,
    )
    .unwrap();
    assert!(named_pipe.is_desktop());
}

#[path = "endpoint/desktop.rs"]
mod desktop;

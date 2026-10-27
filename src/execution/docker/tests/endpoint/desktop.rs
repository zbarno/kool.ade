use super::*;

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
fn desktop_execution_candidates_are_platform_and_endpoint_scoped() {
    let mac = DockerEndpoint::Unix {
        path: "/synthetic/docker.sock".into(),
        desktop: true,
    };
    let windows = DockerEndpoint::NamedPipe {
        path: r"\\.\pipe\docker_engine".into(),
        desktop: true,
    };
    assert!(!mac.supports_application_execution());
    #[cfg(target_os = "windows")]
    assert!(windows.supports_application_execution());
    #[cfg(not(target_os = "windows"))]
    assert!(!windows.supports_application_execution());
    #[cfg(target_os = "macos")]
    {
        let home = std::path::PathBuf::from(std::env::var_os("HOME").unwrap());
        assert!(
            DockerEndpoint::Unix {
                path: home.join(".docker/run/docker.sock"),
                desktop: true,
            }
            .supports_application_execution()
        );
    }
    #[cfg(target_os = "linux")]
    assert_eq!(
        DockerEndpoint::Unix {
            path: format!("/run/user/{}/docker.sock", unsafe { libc::geteuid() }).into(),
            desktop: false,
        }
        .supports_application_execution(),
        unsafe { libc::geteuid() } != 0
    );
    #[cfg(target_os = "linux")]
    assert!(
        !DockerEndpoint::Unix {
            path: "/var/run/docker.sock".into(),
            desktop: false,
        }
        .supports_application_execution()
    );
}

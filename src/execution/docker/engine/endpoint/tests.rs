use super::*;

#[cfg(unix)]
use super::socket::validate_unix_socket;

#[cfg(unix)]
#[test]
fn accepts_only_a_direct_socket_owned_by_the_current_user() {
    use std::os::unix::net::UnixListener;

    let directory = tempfile::tempdir().unwrap();
    let socket_path = directory.path().join("docker.sock");
    let _listener = UnixListener::bind(&socket_path).unwrap();

    validate_unix_socket(&socket_path, false).unwrap();
}

#[cfg(unix)]
#[test]
fn rejects_symlinked_and_non_socket_endpoints() {
    let directory = tempfile::tempdir().unwrap();
    let regular_file = directory.path().join("not-a-socket");
    std::fs::write(&regular_file, "fixture").unwrap();
    let symlink_path = directory.path().join("docker.sock");
    std::os::unix::fs::symlink(&regular_file, &symlink_path).unwrap();

    for path in [&regular_file, &symlink_path] {
        assert_eq!(
            validate_unix_socket(path, false).unwrap_err().code,
            PrerequisiteCode::UntrustedEndpoint
        );
    }
}

#[cfg(target_os = "macos")]
#[test]
fn application_execution_candidate_is_limited_to_desktop_sockets() {
    let home = PathBuf::from(std::env::var_os("HOME").unwrap());
    let approved = home.join(".docker/run/docker.sock");
    assert!(
        DockerEndpoint::Unix {
            path: approved.clone(),
            desktop: true,
        }
        .supports_application_execution()
    );
    assert!(
        !DockerEndpoint::Unix {
            path: home.join(".docker/run/other.sock"),
            desktop: true,
        }
        .supports_application_execution()
    );
    assert!(
        !DockerEndpoint::Unix {
            path: approved,
            desktop: false,
        }
        .supports_application_execution()
    );
}

#[cfg(target_os = "windows")]
#[test]
fn application_execution_candidate_is_limited_to_the_desktop_pipe() {
    assert!(
        DockerEndpoint::NamedPipe {
            path: r"\\.\pipe\docker_engine".into(),
            desktop: true,
        }
        .supports_application_execution()
    );
    assert!(
        !DockerEndpoint::NamedPipe {
            path: r"\\.\pipe\other".into(),
            desktop: true,
        }
        .supports_application_execution()
    );
    assert!(
        !DockerEndpoint::NamedPipe {
            path: r"\\.\pipe\docker_engine".into(),
            desktop: false,
        }
        .supports_application_execution()
    );
}

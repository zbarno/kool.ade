use std::path::{Path, PathBuf};

use bollard::{API_DEFAULT_VERSION, Docker};

use crate::execution::docker::discovery::{PrerequisiteCode, PrerequisiteFailure};

use super::failure;

const API_TIMEOUT_SECONDS: u64 = 5;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HostPlatform {
    Linux,
    MacOs,
    Windows,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DockerEndpoint {
    Unix { path: PathBuf, desktop: bool },
    NamedPipe { path: String, desktop: bool },
}

impl DockerEndpoint {
    pub fn from_process_environment(platform: HostPlatform) -> Result<Self, PrerequisiteFailure> {
        let docker_host = std::env::var("DOCKER_HOST").ok();
        let docker_context = std::env::var("DOCKER_CONTEXT").ok();
        let runtime_dir = std::env::var_os("XDG_RUNTIME_DIR").map(PathBuf::from);
        let home_dir = std::env::var_os("HOME").map(PathBuf::from);
        #[cfg(target_os = "linux")]
        let uid = Some(unsafe { libc::geteuid() });
        #[cfg(not(target_os = "linux"))]
        let uid = None;
        // CI provisions a private, per-job rootless Engine socket outside the
        // operating system's standard Docker locations. Permit that endpoint
        // only in opted-in test binaries; production builds keep the fixed
        // local socket allowlist below.
        let allow_private_test_socket =
            cfg!(test) && std::env::var("KOOLADE_DOCKER_BACKEND_INTEGRATION").as_deref() == Ok("1");
        Self::from_environment_with_test_socket(
            docker_host.as_deref(),
            docker_context.as_deref(),
            runtime_dir.as_deref(),
            home_dir.as_deref(),
            uid,
            platform,
            allow_private_test_socket,
        )
    }

    pub fn from_environment(
        docker_host: Option<&str>,
        docker_context: Option<&str>,
        runtime_dir: Option<&Path>,
        home_dir: Option<&Path>,
        uid: Option<u32>,
        platform: HostPlatform,
    ) -> Result<Self, PrerequisiteFailure> {
        Self::from_environment_with_test_socket(
            docker_host,
            docker_context,
            runtime_dir,
            home_dir,
            uid,
            platform,
            false,
        )
    }

    fn from_environment_with_test_socket(
        docker_host: Option<&str>,
        docker_context: Option<&str>,
        runtime_dir: Option<&Path>,
        home_dir: Option<&Path>,
        uid: Option<u32>,
        platform: HostPlatform,
        allow_private_test_socket: bool,
    ) -> Result<Self, PrerequisiteFailure> {
        if let Some(context) = docker_context.filter(|value| !value.trim().is_empty()) {
            if context == "desktop-linux" {
                return Self::desktop_endpoint(home_dir, platform);
            }
            if context != "default" {
                return Err(failure(
                    PrerequisiteCode::UntrustedEndpoint,
                    "The selected Docker context is not a verified local endpoint; choose the default or Docker Desktop local context",
                    false,
                ));
            }
        }
        if let Some(host) = docker_host.filter(|value| !value.trim().is_empty()) {
            return Self::parse_local_host_for_uid(host, home_dir, uid, allow_private_test_socket);
        }
        match platform {
            HostPlatform::Linux => {
                // `XDG_RUNTIME_DIR` is process environment, so don't let an
                // injected value redirect the privileged Docker API client to
                // an arbitrary socket. The supported native rootless location
                // is the current user's runtime directory only.
                let uid_path = uid.map(|uid| PathBuf::from(format!("/run/user/{uid}/docker.sock")));
                let runtime_path = runtime_dir
                    .zip(uid_path.as_deref())
                    .filter(|(runtime, expected)| *runtime == expected.parent().unwrap_or(expected))
                    .map(|(runtime, _)| runtime.join("docker.sock"));
                let user_path = runtime_path.or(uid_path);
                let rootful_path = PathBuf::from("/var/run/docker.sock");
                let chosen = match user_path {
                    Some(path) if path.exists() || !rootful_path.exists() => path,
                    _ => rootful_path,
                };
                Ok(Self::Unix {
                    path: chosen,
                    desktop: false,
                })
            }
            HostPlatform::MacOs => Self::desktop_endpoint(home_dir, platform),
            HostPlatform::Windows => Ok(Self::NamedPipe {
                path: r"\\.\pipe\docker_engine".into(),
                desktop: true,
            }),
        }
    }

    fn desktop_endpoint(
        home: Option<&Path>,
        platform: HostPlatform,
    ) -> Result<Self, PrerequisiteFailure> {
        match platform {
            HostPlatform::Linux => home.map(|home| Self::Unix {
                path: home.join(".docker/desktop/docker.sock"),
                desktop: true,
            }),
            HostPlatform::MacOs => home.map(|home| Self::Unix {
                path: home.join(".docker/run/docker.sock"),
                desktop: true,
            }),
            HostPlatform::Windows => Some(Self::NamedPipe {
                path: r"\\.\pipe\docker_engine".into(),
                desktop: true,
            }),
        }
        .ok_or_else(|| {
            failure(
                PrerequisiteCode::DockerMissing,
                "A local Docker Desktop endpoint could not be located",
                false,
            )
        })
    }

    pub fn parse_local_host(
        host: &str,
        home_dir: Option<&Path>,
    ) -> Result<Self, PrerequisiteFailure> {
        #[cfg(target_os = "linux")]
        let uid = Some(unsafe { libc::geteuid() });
        #[cfg(not(target_os = "linux"))]
        let uid = None;
        Self::parse_local_host_for_uid(host, home_dir, uid, false)
    }

    fn parse_local_host_for_uid(
        host: &str,
        home_dir: Option<&Path>,
        uid: Option<u32>,
        allow_private_test_socket: bool,
    ) -> Result<Self, PrerequisiteFailure> {
        if let Some(path) = host.strip_prefix("unix://") {
            let path = PathBuf::from(path);
            if !path.is_absolute() || host.contains('@') || host.contains('?') || host.contains('#')
            {
                return Err(failure(
                    PrerequisiteCode::UntrustedEndpoint,
                    "Docker Unix socket address is invalid",
                    false,
                ));
            }
            let rootless_socket = path
                .parent()
                .filter(|parent| {
                    parent
                        .parent()
                        .is_some_and(|parent| parent == Path::new("/run/user"))
                })
                .and_then(Path::file_name)
                .and_then(|name| name.to_str())
                .is_some_and(|name| {
                    !name.is_empty() && name.chars().all(|character| character.is_ascii_digit())
                })
                && uid.is_some_and(|uid| {
                    path.parent().and_then(Path::file_name)
                        == Some(std::ffi::OsStr::new(&uid.to_string()))
                })
                && path.file_name().is_some_and(|name| name == "docker.sock");
            let standard_socket = path == Path::new("/var/run/docker.sock")
                || rootless_socket
                || home_dir.is_some_and(|home| {
                    path == home.join(".docker/desktop/docker.sock")
                        || path == home.join(".docker/run/docker.sock")
                });
            if !standard_socket
                && !(allow_private_test_socket && is_private_test_socket(&path, uid))
            {
                return Err(failure(
                    PrerequisiteCode::UntrustedEndpoint,
                    "Docker Unix socket is outside the supported local Engine locations",
                    false,
                ));
            }
            let desktop = home_dir.is_some_and(|home| {
                path == home.join(".docker/desktop/docker.sock")
                    || path == home.join(".docker/run/docker.sock")
            });
            return Ok(Self::Unix { path, desktop });
        }
        if host == "npipe:////./pipe/docker_engine" || host == r"npipe:\\.\pipe\docker_engine" {
            return Ok(Self::NamedPipe {
                path: r"\\.\pipe\docker_engine".into(),
                desktop: true,
            });
        }
        Err(failure(
            PrerequisiteCode::UntrustedEndpoint,
            "Only a trusted local Unix socket or Docker Desktop named pipe is supported; TCP, HTTP, SSH, and remote contexts are rejected",
            false,
        ))
    }

    pub fn is_desktop(&self) -> bool {
        match self {
            Self::Unix { desktop, .. } | Self::NamedPipe { desktop, .. } => *desktop,
        }
    }

    pub(crate) fn connect(&self) -> Result<Docker, PrerequisiteFailure> {
        let result = match self {
            #[cfg(unix)]
            Self::Unix { path, desktop } => {
                socket::validate_unix_socket(path, *desktop)?;
                Docker::connect_with_unix(
                    &path.to_string_lossy(),
                    API_TIMEOUT_SECONDS,
                    API_DEFAULT_VERSION,
                )
            }
            #[cfg(not(unix))]
            Self::Unix { .. } => {
                return Err(failure(
                    PrerequisiteCode::UntrustedEndpoint,
                    "Docker Unix sockets are unavailable on this platform",
                    false,
                ));
            }
            #[cfg(windows)]
            Self::NamedPipe { path, .. } => {
                Docker::connect_with_named_pipe(path, API_TIMEOUT_SECONDS, API_DEFAULT_VERSION)
            }
            #[cfg(not(windows))]
            Self::NamedPipe { .. } => {
                return Err(failure(
                    PrerequisiteCode::UntrustedEndpoint,
                    "Docker Desktop named pipes are supported only on Windows",
                    false,
                ));
            }
        };
        result.map_err(|error| super::map_connect_error(&error))
    }
}

#[cfg(all(test, target_os = "linux"))]
fn is_private_test_socket(path: &Path, uid: Option<u32>) -> bool {
    use std::os::unix::fs::{FileTypeExt, MetadataExt};

    let Some(uid) = uid else {
        return false;
    };
    let Ok(socket) = std::fs::symlink_metadata(path) else {
        return false;
    };
    let Some(parent) = path.parent() else {
        return false;
    };
    let Ok(directory) = std::fs::symlink_metadata(parent) else {
        return false;
    };
    socket.file_type().is_socket()
        && socket.uid() == uid
        && directory.is_dir()
        && !directory.file_type().is_symlink()
        && directory.uid() == uid
        && directory.mode() & 0o077 == 0
}

#[cfg(not(all(test, target_os = "linux")))]
fn is_private_test_socket(_path: &Path, _uid: Option<u32>) -> bool {
    false
}

#[cfg(unix)]
mod socket;

#[cfg(all(test, unix))]
mod tests {
    use super::socket::validate_unix_socket;
    use super::*;
    use std::os::unix::net::UnixListener;

    #[test]
    fn accepts_only_a_direct_socket_owned_by_the_current_user() {
        let directory = tempfile::tempdir().unwrap();
        let socket_path = directory.path().join("docker.sock");
        let _listener = UnixListener::bind(&socket_path).unwrap();

        validate_unix_socket(&socket_path, false).unwrap();
    }

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

    #[test]
    fn opted_in_tests_accept_only_owned_sockets_in_private_directories() {
        use std::os::unix::fs::PermissionsExt;

        let directory = tempfile::tempdir().unwrap();
        std::fs::set_permissions(directory.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
        let socket_path = directory.path().join("docker.sock");
        let _listener = UnixListener::bind(&socket_path).unwrap();
        let uid = unsafe { libc::geteuid() };
        let endpoint = DockerEndpoint::from_environment_with_test_socket(
            Some(&format!("unix://{}", socket_path.display())),
            None,
            None,
            None,
            Some(uid),
            HostPlatform::Linux,
            true,
        )
        .unwrap();
        assert_eq!(
            endpoint,
            DockerEndpoint::Unix {
                path: socket_path.clone(),
                desktop: false,
            }
        );

        std::fs::set_permissions(directory.path(), std::fs::Permissions::from_mode(0o755)).unwrap();
        assert_eq!(
            DockerEndpoint::from_environment_with_test_socket(
                Some(&format!("unix://{}", socket_path.display())),
                None,
                None,
                None,
                Some(uid),
                HostPlatform::Linux,
                true,
            )
            .unwrap_err()
            .code,
            PrerequisiteCode::UntrustedEndpoint
        );
    }
}

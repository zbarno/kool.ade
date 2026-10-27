use std::path::{Path, PathBuf};

use crate::execution::docker::discovery::{PrerequisiteCode, PrerequisiteFailure};

use super::failure;

#[cfg(unix)]
const API_TIMEOUT_SECONDS: u64 = 5;

mod connection;
mod context;
#[cfg(target_os = "macos")]
mod macos_app;
#[cfg(windows)]
mod named_pipe;
#[cfg(unix)]
mod pinned;
#[cfg(test)]
mod test_support;

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
        let endpoint = Self::from_environment(
            docker_host.as_deref(),
            docker_context.as_deref(),
            runtime_dir.as_deref(),
            home_dir.as_deref(),
            uid,
            platform,
        )?;
        #[cfg(target_os = "macos")]
        if platform == HostPlatform::MacOs {
            macos_app::verify_installed_docker_desktop()?;
        }
        Ok(endpoint)
    }

    #[cfg(test)]
    pub(crate) fn from_process_environment_with_test_support(
        platform: HostPlatform,
    ) -> Result<Self, PrerequisiteFailure> {
        if let Some(endpoint) = Self::test_support_endpoint_from_process_environment(platform) {
            return Ok(endpoint);
        }
        Self::from_process_environment(platform)
    }

    pub fn from_environment(
        docker_host: Option<&str>,
        docker_context: Option<&str>,
        _runtime_dir: Option<&Path>,
        home_dir: Option<&Path>,
        uid: Option<u32>,
        platform: HostPlatform,
    ) -> Result<Self, PrerequisiteFailure> {
        let context_config_dir = context::docker_config_dir(home_dir);
        Self::from_environment_with_config(
            docker_host,
            docker_context,
            home_dir,
            uid,
            platform,
            context_config_dir.as_deref(),
        )
    }

    pub(crate) fn from_environment_with_config(
        docker_host: Option<&str>,
        docker_context: Option<&str>,
        home_dir: Option<&Path>,
        uid: Option<u32>,
        platform: HostPlatform,
        context_config_dir: Option<&Path>,
    ) -> Result<Self, PrerequisiteFailure> {
        if platform == HostPlatform::Linux {
            return context::linux_rootless_endpoint(
                docker_host,
                docker_context,
                uid,
                context_config_dir,
            );
        }
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
            return Self::parse_local_host_for_uid(host, home_dir, uid);
        }
        match platform {
            HostPlatform::Linux => unreachable!("Linux endpoint was resolved above"),
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
        _home_dir: Option<&Path>,
    ) -> Result<Self, PrerequisiteFailure> {
        #[cfg(target_os = "linux")]
        {
            context::linux_rootless_endpoint(
                Some(host),
                None,
                Some(unsafe { libc::geteuid() }),
                None,
            )
        }
        #[cfg(not(target_os = "linux"))]
        {
            Self::parse_local_host_for_uid(host, _home_dir, None)
        }
    }

    fn parse_local_host_for_uid(
        host: &str,
        home_dir: Option<&Path>,
        uid: Option<u32>,
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
            #[cfg(test)]
            let test_socket = test_support::isolated_rootless_socket(&path);
            #[cfg(not(test))]
            let test_socket = false;
            let standard_socket = path == Path::new("/var/run/docker.sock")
                || rootless_socket
                || home_dir.is_some_and(|home| {
                    path == home.join(".docker/desktop/docker.sock")
                        || path == home.join(".docker/run/docker.sock")
                })
                || test_socket;
            if !standard_socket {
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

    pub fn supports_application_execution(&self) -> bool {
        #[cfg(target_os = "linux")]
        {
            let current_uid = unsafe { libc::geteuid() };
            current_uid != 0
                && matches!(
                    self,
                    Self::Unix { path, desktop: false }
                        if path == &PathBuf::from(format!("/run/user/{current_uid}/docker.sock"))
                            || {
                                #[cfg(test)]
                                {
                                    test_support::isolated_rootless_socket(path)
                                }
                                #[cfg(not(test))]
                                {
                                    false
                                }
                            }
                )
        }
        #[cfg(target_os = "macos")]
        {
            let Some(home) = std::env::var_os("HOME").map(PathBuf::from) else {
                return false;
            };
            matches!(
                self,
                Self::Unix {
                    path,
                    desktop: true
                } if path == &home.join(".docker/run/docker.sock")
                    || path == &home.join(".docker/desktop/docker.sock")
            )
        }
        #[cfg(target_os = "windows")]
        {
            matches!(
                self,
                Self::NamedPipe {
                    path,
                    desktop: true
                } if path == r"\\.\pipe\docker_engine"
            )
        }
        #[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
        {
            false
        }
    }
}

#[cfg(unix)]
pub(crate) mod socket;

#[cfg(test)]
#[path = "endpoint/tests.rs"]
mod tests;

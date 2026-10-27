#[cfg(unix)]
use bollard::API_DEFAULT_VERSION;
use bollard::Docker;

use crate::execution::docker::discovery::{PrerequisiteCode, PrerequisiteFailure};

use super::DockerEndpoint;

impl DockerEndpoint {
    pub(crate) fn identity(&self) -> String {
        match self {
            Self::Unix { path, desktop } => format!("unix:{}:{desktop}", path.display()),
            Self::NamedPipe { path, desktop } => format!("named-pipe:{path}:{desktop}"),
        }
    }

    pub(crate) fn fingerprint(&self) -> String {
        use sha2::Digest as _;
        format!("{:x}", sha2::Sha256::digest(self.identity().as_bytes()))
    }

    pub(crate) fn connect(&self) -> Result<Docker, PrerequisiteFailure> {
        #[cfg(windows)]
        {
            return match self {
                Self::NamedPipe { path, .. } => super::named_pipe::connect(path),
                Self::Unix { .. } => Err(super::super::failure(
                    PrerequisiteCode::UntrustedEndpoint,
                    "Docker Unix sockets are unavailable on this platform",
                    false,
                )),
            };
        }

        #[cfg(not(windows))]
        {
            let result = match self {
                #[cfg(unix)]
                Self::Unix { path, desktop } => {
                    super::socket::validate_unix_socket(path, *desktop)?;
                    #[cfg(target_os = "macos")]
                    if *desktop {
                        return super::pinned::connect(path, *desktop)
                            .map_err(|error| super::super::map_connect_error(&error));
                    }
                    Docker::connect_with_unix(
                        &path.to_string_lossy(),
                        super::API_TIMEOUT_SECONDS,
                        API_DEFAULT_VERSION,
                    )
                }
                #[cfg(not(unix))]
                Self::Unix { .. } => {
                    return Err(super::super::failure(
                        PrerequisiteCode::UntrustedEndpoint,
                        "Docker Unix sockets are unavailable on this platform",
                        false,
                    ));
                }
                Self::NamedPipe { .. } => {
                    return Err(super::super::failure(
                        PrerequisiteCode::UntrustedEndpoint,
                        "Docker Desktop named pipes are supported only on Windows",
                        false,
                    ));
                }
            };
            result.map_err(|error| super::super::map_connect_error(&error))
        }
    }

    #[cfg(unix)]
    pub(crate) fn connect_pinned(&self) -> Result<Docker, PrerequisiteFailure> {
        match self {
            Self::Unix { path, desktop } => {
                super::socket::validate_unix_socket(path, *desktop)?;
                super::pinned::connect(path, *desktop)
                    .map_err(|error| super::super::map_connect_error(&error))
            }
            Self::NamedPipe { .. } => Err(super::super::failure(
                PrerequisiteCode::UntrustedEndpoint,
                "A persistent pinned Docker API connection is unavailable for this endpoint",
                false,
            )),
        }
    }
}

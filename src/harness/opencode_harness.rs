//! OpenCode CLI adapter and bounded local readiness probe.

use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::error::AppError;

pub const OPENCODE_BINARY_ENV: &str = "KOOLADE_OPENCODE_BIN";
const PROBE_TIMEOUT: Duration = Duration::from_secs(12);

mod execute;
mod readiness;
mod stream;
#[cfg(test)]
pub(super) use execute::prompt_input;
use readiness::{probe, probe_version};
#[cfg(test)]
use stream::{OpenCodeEvent, parse_event};

#[derive(Default)]
pub struct OpenCodeHarness;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OpenCodeReadiness {
    Missing,
    InvalidInstallation,
    AuthenticationRequired,
    ConfigurationRequired,
    CatalogUnavailable,
    Ready,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpenCodeProbeReport {
    pub status: String,
    pub version: Option<String>,
    pub diagnostic: String,
    pub binary: Option<PathBuf>,
    pub readiness: OpenCodeReadiness,
    pub models: Vec<String>,
}

impl OpenCodeHarness {
    pub fn locate_binary() -> Result<PathBuf, AppError> {
        if let Some(path) = crate::harness::manual_executable_path("opencode")? {
            return Ok(path);
        }
        if let Ok(value) = std::env::var(OPENCODE_BINARY_ENV)
            && !value.trim().is_empty()
        {
            let path = PathBuf::from(value);
            if executable(&path) {
                return Ok(path);
            }
            return Err(AppError::HarnessNotFound {
                detail: format!("{OPENCODE_BINARY_ENV} does not name an executable file"),
            });
        }
        if let Some(paths) = std::env::var_os("PATH") {
            for directory in std::env::split_paths(&paths) {
                let candidate = directory.join("opencode");
                if executable(&candidate) {
                    return Ok(candidate);
                }
            }
        }
        if let Some(home) = std::env::var_os("HOME") {
            for site in [".npm-global/bin", ".local/bin", ".opencode/bin"] {
                let candidate = PathBuf::from(&home).join(site).join("opencode");
                if executable(&candidate) {
                    return Ok(candidate);
                }
            }
        }
        Err(AppError::HarnessNotFound {
            detail: format!(
                "opencode executable not found in PATH or common install locations; set {OPENCODE_BINARY_ENV} to override"
            ),
        })
    }

    pub fn probe_report() -> OpenCodeProbeReport {
        let binary = match Self::locate_binary() {
            Ok(path) => path,
            Err(error) => {
                return OpenCodeProbeReport {
                    status: "opencode (not installed)".into(),
                    version: None,
                    diagnostic: error.detail(),
                    binary: None,
                    readiness: OpenCodeReadiness::Missing,
                    models: Vec::new(),
                };
            }
        };
        match probe(&binary, PROBE_TIMEOUT) {
            Ok(outcome) => OpenCodeProbeReport {
                status: match outcome.readiness {
                    OpenCodeReadiness::Ready => format!("opencode {}", outcome.version),
                    OpenCodeReadiness::CatalogUnavailable => {
                        format!("opencode {} (model catalog unavailable)", outcome.version)
                    }
                    OpenCodeReadiness::AuthenticationRequired => {
                        "opencode (authentication required)".into()
                    }
                    OpenCodeReadiness::ConfigurationRequired => {
                        "opencode (configuration required)".into()
                    }
                    OpenCodeReadiness::Missing | OpenCodeReadiness::InvalidInstallation => {
                        "opencode (unavailable)".into()
                    }
                },
                version: Some(outcome.version),
                diagnostic: outcome.diagnostic,
                binary: Some(binary),
                readiness: outcome.readiness,
                models: outcome.models,
            },
            Err((readiness, diagnostic, version)) => OpenCodeProbeReport {
                status: match readiness {
                    OpenCodeReadiness::InvalidInstallation => "opencode (unavailable)".into(),
                    OpenCodeReadiness::AuthenticationRequired => {
                        "opencode (authentication required)".into()
                    }
                    OpenCodeReadiness::ConfigurationRequired => {
                        "opencode (configuration required)".into()
                    }
                    _ => "opencode (unavailable)".into(),
                },
                version,
                diagnostic,
                binary: Some(binary),
                readiness,
                models: Vec::new(),
            },
        }
    }

    pub(super) fn checked_binary(path: &Path) -> Result<String, AppError> {
        let version = probe_version(path, PROBE_TIMEOUT).map_err(AppError::Other)?;
        crate::harness::execution_security::ProfileProbeEvidence::probe(
            crate::harness::execution_security::CliProvider::OpenCode,
            path,
            version.clone(),
        )
        .map_err(|error| AppError::Other(error.to_string()))?;
        Ok(version)
    }
}

fn executable(path: &Path) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        path.metadata()
            .is_ok_and(|metadata| metadata.is_file() && metadata.permissions().mode() & 0o111 != 0)
    }
    #[cfg(not(unix))]
    {
        path.is_file()
    }
}

#[cfg(test)]
#[path = "opencode_harness/tests.rs"]
mod tests;

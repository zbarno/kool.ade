//! Headless Google Antigravity CLI adapter.

use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::error::AppError;

pub const ANTIGRAVITY_BINARY_ENV: &str = "KOOLADE_ANTIGRAVITY_BIN";
pub const ANTIGRAVITY_MODEL_ENV: &str = "KOOLADE_ANTIGRAVITY_MODEL";
const PROBE_TIMEOUT: Duration = Duration::from_secs(10);

mod execute;
mod probe;
#[cfg(test)]
mod tests;
#[cfg(test)]
use probe::probe_with_timeouts;
use probe::{probe, probe_version};

#[derive(Default)]
pub struct AntigravityHarness;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AntigravityProbeReport {
    pub status: String,
    pub version: Option<String>,
    pub diagnostic: String,
    pub binary: Option<PathBuf>,
    pub ready: bool,
    pub configuration_required: bool,
    pub models: Vec<String>,
}

impl AntigravityHarness {
    pub fn locate_binary() -> Result<PathBuf, AppError> {
        if let Some(path) = crate::harness::manual_executable_path("antigravity")? {
            return Ok(path);
        }
        if let Ok(value) = std::env::var(ANTIGRAVITY_BINARY_ENV)
            && !value.trim().is_empty()
        {
            let path = PathBuf::from(value);
            return executable(&path)
                .then_some(path)
                .ok_or_else(|| AppError::HarnessNotFound {
                    detail: format!("{ANTIGRAVITY_BINARY_ENV} does not name an executable file"),
                });
        }
        if let Some(paths) = std::env::var_os("PATH") {
            for directory in std::env::split_paths(&paths) {
                let path = directory.join("agy");
                if executable(&path) {
                    return Ok(path);
                }
            }
        }
        if let Some(home) = std::env::var_os("HOME") {
            for site in [".local/bin", ".npm-global/bin"] {
                let path = PathBuf::from(&home).join(site).join("agy");
                if executable(&path) {
                    return Ok(path);
                }
            }
        }
        Err(AppError::HarnessNotFound {
            detail: format!(
                "agy executable not found in PATH or common install locations; set {ANTIGRAVITY_BINARY_ENV} to override"
            ),
        })
    }

    pub fn probe_report() -> AntigravityProbeReport {
        let binary = match Self::locate_binary() {
            Ok(path) => path,
            Err(error) => {
                return AntigravityProbeReport {
                    status: "Antigravity (not installed)".into(),
                    version: None,
                    diagnostic: error.detail(),
                    binary: None,
                    ready: false,
                    configuration_required: false,
                    models: Vec::new(),
                };
            }
        };
        match probe(&binary, PROBE_TIMEOUT) {
            Ok(outcome) => {
                let status = if outcome.configuration_required {
                    "Antigravity (authentication required)".into()
                } else if outcome.catalog_diagnostic.is_some() {
                    format!(
                        "Antigravity {} (model catalog unavailable)",
                        outcome.version
                    )
                } else {
                    format!("Antigravity {}", outcome.version)
                };
                let mut diagnostic =
                    "Authentication and provider access are checked when a headless task runs."
                        .to_owned();
                if outcome.configuration_required {
                    diagnostic =
                        "Antigravity authentication is required. Sign in and retry.".into();
                }
                if let Some(catalog_diagnostic) = outcome.catalog_diagnostic {
                    diagnostic.push(' ');
                    diagnostic.push_str(&catalog_diagnostic);
                }
                AntigravityProbeReport {
                    status,
                    version: Some(outcome.version),
                    diagnostic,
                    binary: Some(binary),
                    ready: !outcome.configuration_required,
                    configuration_required: outcome.configuration_required,
                    models: outcome.models,
                }
            }
            Err((diagnostic, version)) => AntigravityProbeReport {
                status: "Antigravity (unavailable)".into(),
                version,
                diagnostic,
                binary: Some(binary),
                ready: false,
                configuration_required: false,
                models: Vec::new(),
            },
        }
    }

    pub(super) fn checked_binary(path: &Path) -> Result<String, AppError> {
        let version =
            probe_version(path, PROBE_TIMEOUT).map_err(|(message, _)| AppError::Other(message))?;
        crate::harness::execution_security::ProfileProbeEvidence::probe(
            crate::harness::execution_security::CliProvider::Antigravity,
            path,
            version.clone(),
        )
        .map_err(|error| AppError::Other(error.to_string()))?;
        Ok(version)
    }
}

fn parse_models(text: &str) -> Vec<String> {
    let mut models = text
        .lines()
        .filter_map(|line| {
            let slug = line.split_whitespace().next()?;
            (slug.contains('-')
                && slug
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '.' | '_')))
            .then(|| slug.to_owned())
        })
        .collect::<Vec<_>>();
    models.sort();
    models.dedup();
    models
}

fn executable(path: &Path) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        path.metadata()
            .is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
    }
    #[cfg(not(unix))]
    {
        path.is_file()
    }
}

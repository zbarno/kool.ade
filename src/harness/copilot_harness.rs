//! GitHub Copilot CLI adapter for task repositories.

use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::error::AppError;

pub const COPILOT_BINARY_ENV: &str = "KOOLADE_COPILOT_BIN";
pub const COPILOT_MODEL_ENV: &str = "KOOLADE_COPILOT_MODEL";
mod execute;
#[cfg(test)]
mod tests;
#[cfg(test)]
pub(super) use execute::prompt_input;

#[derive(Default)]
pub struct CopilotHarness;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CopilotReadiness {
    Missing,
    Unusable,
    Ready,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CopilotProbeReport {
    pub binary: Option<PathBuf>,
    pub version: Option<String>,
    pub readiness: CopilotReadiness,
    pub diagnostic: String,
}

impl CopilotHarness {
    pub fn locate_binary() -> Result<PathBuf, AppError> {
        if let Some(path) = crate::harness::manual_executable_path("copilot")? {
            return Ok(path);
        }
        if let Ok(value) = std::env::var(COPILOT_BINARY_ENV)
            && !value.trim().is_empty()
        {
            let path = PathBuf::from(value);
            if executable(&path) {
                return Ok(path);
            }
            return Err(AppError::HarnessNotFound {
                detail: format!("{COPILOT_BINARY_ENV} does not name an executable file"),
            });
        }
        for candidate in crate::harness::discovery::host_executable_candidates("copilot") {
            if executable(&candidate) {
                return Ok(candidate);
            }
        }
        Err(AppError::HarnessNotFound {
            detail: "GitHub Copilot CLI was not found in PATH or common install locations".into(),
        })
    }

    pub fn probe_report() -> CopilotProbeReport {
        let binary = match Self::locate_binary() {
            Ok(path) => path,
            Err(error) => {
                return CopilotProbeReport {
                    binary: None,
                    version: None,
                    readiness: CopilotReadiness::Missing,
                    diagnostic: error.detail(),
                };
            }
        };
        match run(&binary, &["--version"]) {
            Ok((output, true)) => {
                if let Some(version) = copilot_version(&output) {
                    CopilotProbeReport {
                        binary: Some(binary),
                        version: Some(version),
                        readiness: CopilotReadiness::Ready,
                        diagnostic: String::new(),
                    }
                } else {
                    CopilotProbeReport {
                        binary: Some(binary),
                        version: None,
                        readiness: CopilotReadiness::Unusable,
                        diagnostic: "Executable did not identify itself as GitHub Copilot CLI with a version from --version".into(),
                    }
                }
            }
            Ok(_) => CopilotProbeReport {
                binary: Some(binary),
                version: None,
                readiness: CopilotReadiness::Unusable,
                diagnostic: "GitHub Copilot CLI version command failed".into(),
            },
            Err(error) => CopilotProbeReport {
                binary: Some(binary),
                version: None,
                readiness: CopilotReadiness::Unusable,
                diagnostic: error,
            },
        }
    }
}

fn copilot_version(output: &str) -> Option<String> {
    crate::harness::discovery::normalize_cli_version(
        output,
        &["GitHub Copilot CLI", "Copilot CLI"],
        false,
    )
}

fn executable(path: &Path) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        path.is_file()
            && std::fs::metadata(path)
                .is_ok_and(|metadata| metadata.permissions().mode() & 0o111 != 0)
    }
    #[cfg(windows)]
    {
        path.is_file()
            && path
                .extension()
                .is_some_and(|extension| extension.eq_ignore_ascii_case("exe"))
    }
}

fn run(binary: &Path, args: &[&str]) -> Result<(String, bool), String> {
    let argv = std::iter::once(binary.to_string_lossy().into_owned())
        .chain(args.iter().map(|arg| (*arg).to_owned()))
        .collect::<Vec<_>>();
    let output = crate::harness::discovery::run(
        &argv,
        Duration::from_secs(10),
        crate::harness::discovery::MAX_PROBE_OUTPUT_BYTES,
        crate::harness::discovery::MAX_PROBE_OUTPUT_BYTES,
        crate::harness::execution_security::CliProvider::Copilot.excluded_child_environment(),
    )
    .map_err(|error| format!("GitHub Copilot CLI readiness check failed: {error}"))?;
    if output.stdout_truncated || output.stderr_truncated {
        return Err("GitHub Copilot CLI readiness output exceeded the size limit".into());
    }
    Ok((
        String::from_utf8_lossy(&output.stdout).into_owned(),
        output.success,
    ))
}

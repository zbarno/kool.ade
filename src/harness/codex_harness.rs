//! OpenAI Codex CLI adapter. Codex's flags, JSONL event parsing, and readiness
//! checks stay behind the shared [`AiHarness`] interface.

use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::error::AppError;
pub const CODEX_BINARY_ENV: &str = "KOOLADE_CODEX_BIN";
pub const CODEX_MODEL_ENV: &str = "KOOLADE_CODEX_MODEL";
mod execute;
#[cfg(test)]
pub(super) use execute::{CodexEvent, command, normalize_effort, parse_event};

const PROBE_TIMEOUT: Duration = Duration::from_secs(10);
const STDERR_LIMIT: usize = 100;

#[derive(Default)]
pub struct CodexHarness;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodexProbeReport {
    pub status: String,
    pub version: Option<String>,
    pub diagnostic: String,
    pub binary: Option<PathBuf>,
    pub ready: bool,
    pub readiness: CodexReadiness,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CodexReadiness {
    Missing,
    Unusable,
    AuthenticationRequired,
    Ready,
}

impl CodexHarness {
    pub fn locate_binary() -> Result<PathBuf, AppError> {
        if let Some(path) = crate::harness::manual_executable_path("codex")? {
            return Ok(path);
        }
        if let Ok(value) = std::env::var(CODEX_BINARY_ENV)
            && !value.trim().is_empty()
        {
            let path = PathBuf::from(&value);
            if executable(&path) {
                return Ok(path);
            }
            return Err(AppError::HarnessNotFound {
                detail: format!("{CODEX_BINARY_ENV} does not name an executable file"),
            });
        }
        if let Some(paths) = std::env::var_os("PATH") {
            for directory in std::env::split_paths(&paths) {
                let candidate = directory.join("codex");
                if executable(&candidate) {
                    return Ok(candidate);
                }
            }
        }
        if let Some(home) = std::env::var_os("HOME") {
            for site in [".npm-global/bin", ".local/bin"] {
                let candidate = PathBuf::from(&home).join(site).join("codex");
                if executable(&candidate) {
                    return Ok(candidate);
                }
            }
        }
        Err(AppError::HarnessNotFound {
            detail: format!(
                "codex executable not found in PATH or common install locations; set {CODEX_BINARY_ENV} to override"
            ),
        })
    }

    pub fn probe_report() -> CodexProbeReport {
        let path = match Self::locate_binary() {
            Ok(path) => path,
            Err(error) => {
                return CodexProbeReport {
                    status: "codex (not installed)".into(),
                    version: None,
                    diagnostic: error.detail(),
                    binary: None,
                    ready: false,
                    readiness: CodexReadiness::Missing,
                };
            }
        };
        let version = run_probe(&path, &["--version"]).ok().and_then(|output| {
            crate::harness::discovery::normalize_cli_version(&output, &["codex-cli"], false)
        });
        match Self::check_binary(&path) {
            Ok(version) => CodexProbeReport {
                status: format!("codex {version}"),
                version: Some(version),
                diagnostic: String::new(),
                binary: Some(path),
                ready: true,
                readiness: CodexReadiness::Ready,
            },
            Err(error) => {
                let readiness = if error.detail().contains("not authenticated") {
                    CodexReadiness::AuthenticationRequired
                } else {
                    CodexReadiness::Unusable
                };
                CodexProbeReport {
                    status: match readiness {
                        CodexReadiness::AuthenticationRequired => {
                            "codex (authentication required)".into()
                        }
                        _ => "codex (installed but unusable)".into(),
                    },
                    version,
                    diagnostic: error.detail(),
                    binary: Some(path),
                    ready: false,
                    readiness,
                }
            }
        }
    }

    pub(super) fn check_binary(path: &Path) -> Result<String, AppError> {
        let version = crate::harness::discovery::normalize_cli_version(
            &run_probe(path, &["--version"])?,
            &["codex-cli"],
            false,
        )
        .ok_or_else(|| AppError::Other("Codex CLI did not report a safe version".into()))?;
        #[cfg(test)]
        check_supported_capabilities(path)?;
        #[cfg(not(test))]
        crate::harness::execution_security::ProfileProbeEvidence::probe(
            crate::harness::execution_security::CliProvider::Codex,
            path,
            version.clone(),
        )
        .map_err(|error| AppError::Other(error.to_string()))?;
        let login = run_probe_status(path, &["login", "status"]);
        match login {
            Ok((output, true))
                if output.to_ascii_lowercase().contains("logged in")
                    && !output.to_ascii_lowercase().contains("not logged in") =>
            {
                Ok(version)
            }
            Ok((output, _))
                if output.to_ascii_lowercase().contains("not logged in")
                    || output.to_ascii_lowercase().contains("logged out")
                    || output.to_ascii_lowercase().contains("not authenticated") =>
            {
                Err(AppError::Other(
                    "Codex CLI is installed but is not authenticated. Run `codex login` and retry."
                        .into(),
                ))
            }
            Ok((_, false)) => Err(AppError::Other(
                "Codex CLI login status could not be checked because the command failed.".into(),
            )),
            Ok((_, true)) => Err(AppError::Other(
                "Codex CLI login status returned an unrecognized response.".into(),
            )),
            Err(error) => Err(AppError::Other(format!(
                "Codex CLI login status could not be checked: {}",
                error.detail()
            ))),
        }
    }
}

#[cfg(test)]
fn check_supported_capabilities(path: &Path) -> Result<(), AppError> {
    let argv = [
        path.to_string_lossy().into_owned(),
        "exec".into(),
        "--help".into(),
    ];
    let output = crate::harness::discovery::run_isolated(
        &argv,
        PROBE_TIMEOUT,
        crate::harness::discovery::MAX_PROBE_OUTPUT_BYTES,
        crate::harness::discovery::MAX_PROBE_OUTPUT_BYTES,
        crate::harness::execution_security::CliProvider::Codex.excluded_child_environment(),
    )
    .map_err(|_| AppError::Other("Codex CLI capability check failed or timed out".into()))?;
    if !output.success || output.stdout_truncated || output.stderr_truncated {
        return Err(AppError::Other(
            "Codex CLI capability check failed or exceeded the output limit".into(),
        ));
    }
    let help = format!(
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let required = [
        "--json",
        "--cd",
        "--sandbox",
        "--ephemeral",
        "--model",
        "--config",
        "--disable",
        "--ignore-user-config",
        "--skip-git-repo-check",
    ];
    let missing = required
        .into_iter()
        .filter(|option| !help.contains(option))
        .collect::<Vec<_>>();
    if missing.is_empty() {
        Ok(())
    } else {
        Err(AppError::Other(format!(
            "Codex CLI is too old or lacks required exec options: {}",
            missing.join(", ")
        )))
    }
}

fn run_probe(binary: &Path, args: &[&str]) -> Result<String, AppError> {
    let (output, success) = run_probe_status(binary, args)?;
    if success {
        Ok(output)
    } else {
        Err(AppError::Other("Codex CLI readiness check failed".into()))
    }
}

fn run_probe_status(binary: &Path, args: &[&str]) -> Result<(String, bool), AppError> {
    let mut argv = vec![binary.to_string_lossy().into_owned()];
    argv.extend(args.iter().map(|arg| (*arg).to_owned()));
    let output = crate::harness::discovery::run(
        &argv,
        PROBE_TIMEOUT,
        crate::harness::discovery::MAX_PROBE_OUTPUT_BYTES,
        crate::harness::discovery::MAX_PROBE_OUTPUT_BYTES,
        crate::harness::execution_security::CliProvider::Codex.excluded_child_environment(),
    )
    .map_err(|_| AppError::Other("Codex CLI readiness check failed or timed out".into()))?;
    if output.stdout_truncated || output.stderr_truncated {
        return Err(AppError::Other(
            "Codex CLI readiness output exceeded the size limit".into(),
        ));
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    Ok((format!("{stdout}\n{stderr}"), output.success))
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
#[path = "codex_harness/tests.rs"]
mod tests;

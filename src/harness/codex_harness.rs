//! OpenAI Codex CLI adapter. Codex's flags, JSONL event parsing, and readiness
//! checks stay behind the shared [`AiHarness`] interface.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use crate::error::AppError;
use crate::harness::pi_proc::{PollState, StreamEvt};
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
            output
                .lines()
                .next()
                .map(str::trim)
                .filter(|line| !line.is_empty())
                .map(str::to_owned)
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
        let version = run_probe(path, &["--version"])?;
        check_supported_capabilities(path)?;
        let login = run_probe_status(path, &["login", "status"]);
        match login {
            Ok((output, true))
                if output.to_ascii_lowercase().contains("logged in")
                    && !output.to_ascii_lowercase().contains("not logged in") =>
            {
                let version = version.lines().next().unwrap_or_default().trim();
                if version.is_empty() {
                    return Err(AppError::Other("Codex CLI did not report a version".into()));
                }
                Ok(version.to_owned())
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

fn check_supported_capabilities(path: &Path) -> Result<(), AppError> {
    let help = run_probe(path, &["exec", "--help"])?;
    let required = [
        "--json",
        "--cd",
        "--sandbox",
        "--ephemeral",
        "--model",
        "--config",
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
    let task = crate::harness::pi_proc::spawn(&argv, Path::new("."))?;
    let deadline = Instant::now() + PROBE_TIMEOUT;
    let mut output = String::new();
    let mut success = None;
    while Instant::now() < deadline {
        match task.poll_next(Duration::from_millis(100)) {
            Ok(StreamEvt::Stdout(line)) => {
                output.push_str(&line);
                output.push('\n');
            }
            Ok(StreamEvt::Stderr(_)) => {}
            Ok(StreamEvt::Exited(ok)) => {
                success = Some(ok);
                break;
            }
            Err(PollState::Closed) => break,
            Err(PollState::Pending) => {}
        }
    }
    match success {
        Some(success) => Ok((output, success)),
        None => {
            task.kill();
            let _ = task.settle(Duration::from_secs(1));
            Err(AppError::Other(
                "Codex CLI readiness check timed out".into(),
            ))
        }
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
#[path = "codex_harness/tests.rs"]
mod tests;

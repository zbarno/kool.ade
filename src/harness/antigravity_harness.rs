//! Headless Google Antigravity CLI adapter.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use crate::error::AppError;
use crate::harness::pi_proc::{PollState, StreamEvt};

pub const ANTIGRAVITY_BINARY_ENV: &str = "KOOLADE_ANTIGRAVITY_BIN";
pub const ANTIGRAVITY_MODEL_ENV: &str = "KOOLADE_ANTIGRAVITY_MODEL";
const PROBE_TIMEOUT: Duration = Duration::from_secs(10);

mod execute;
#[cfg(test)]
mod tests;

#[derive(Default)]
pub struct AntigravityHarness;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AntigravityProbeReport {
    pub status: String,
    pub version: Option<String>,
    pub diagnostic: String,
    pub binary: Option<PathBuf>,
    pub ready: bool,
    pub models: Vec<String>,
}

impl AntigravityHarness {
    pub fn locate_binary() -> Result<PathBuf, AppError> {
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
                    models: Vec::new(),
                };
            }
        };
        match probe(&binary) {
            Ok((version, models)) => AntigravityProbeReport {
                status: format!("Antigravity {version}"),
                version: Some(version),
                diagnostic:
                    "Authentication and provider access are checked when a headless task runs."
                        .into(),
                binary: Some(binary),
                ready: true,
                models,
            },
            Err((diagnostic, version)) => AntigravityProbeReport {
                status: "Antigravity (unavailable)".into(),
                version,
                diagnostic,
                binary: Some(binary),
                ready: false,
                models: Vec::new(),
            },
        }
    }

    pub(super) fn checked_binary(path: &Path) -> Result<String, AppError> {
        probe(path)
            .map(|(version, _)| version)
            .map_err(|(message, _)| AppError::Other(message))
    }
}

fn probe(binary: &Path) -> Result<(String, Vec<String>), (String, Option<String>)> {
    let (output, success) = run(binary, &["--version"])?;
    if !success {
        return Err(("Antigravity version command failed".into(), None));
    }
    let version = output.lines().next().unwrap_or_default().trim().to_owned();
    if version.is_empty() {
        return Err(("Antigravity did not report a version".into(), None));
    }
    let (models, model_success) = run(binary, &["models"])?;
    if !model_success {
        return Err(("Antigravity model discovery failed".into(), Some(version)));
    }
    Ok((version, parse_models(&models)))
}

fn run(binary: &Path, args: &[&str]) -> Result<(String, bool), (String, Option<String>)> {
    let mut argv = vec![binary.to_string_lossy().into_owned()];
    argv.extend(args.iter().map(|arg| (*arg).to_owned()));
    let task = crate::harness::pi_proc::spawn(&argv, Path::new("."))
        .map_err(|e| (format!("Antigravity CLI could not start: {e}"), None))?;
    let deadline = Instant::now() + PROBE_TIMEOUT;
    let mut stdout = String::new();
    let mut exit = None;
    while Instant::now() < deadline {
        match task.poll_next(Duration::from_millis(100)) {
            Ok(StreamEvt::Stdout(line)) => {
                stdout.push_str(&line);
                stdout.push('\n');
            }
            Ok(StreamEvt::Stderr(_)) => {}
            Ok(StreamEvt::Exited(success)) => exit = Some(success),
            Err(PollState::Closed) => break,
            Err(PollState::Pending) => {}
        }
    }
    match exit {
        Some(success) => Ok((stdout, success)),
        None => {
            task.kill();
            let _ = task.settle(Duration::from_secs(1));
            Err(("Antigravity CLI readiness check timed out".into(), None))
        }
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

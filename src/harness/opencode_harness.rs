//! OpenCode CLI adapter and bounded local readiness probe.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use crate::error::AppError;
use crate::harness::pi_proc::{PollState, StreamEvt};

pub const OPENCODE_BINARY_ENV: &str = "KOOLADE_OPENCODE_BIN";
const PROBE_TIMEOUT: Duration = Duration::from_secs(12);

mod execute;
mod stream;
#[cfg(test)]
use stream::{OpenCodeEvent, parse_event, permission_policy};

#[derive(Default)]
pub struct OpenCodeHarness;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OpenCodeReadiness {
    Missing,
    InvalidInstallation,
    AuthenticationRequired,
    ConfigurationRequired,
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
        match probe(&binary) {
            Ok((version, models)) => OpenCodeProbeReport {
                status: format!("opencode {version}"),
                version: Some(version),
                diagnostic: String::new(),
                binary: Some(binary),
                readiness: OpenCodeReadiness::Ready,
                models,
            },
            Err((readiness, diagnostic)) => OpenCodeProbeReport {
                status: match readiness {
                    OpenCodeReadiness::InvalidInstallation => "opencode (unavailable)".into(),
                    OpenCodeReadiness::AuthenticationRequired => {
                        "opencode (authentication required)".into()
                    }
                    OpenCodeReadiness::ConfigurationRequired => {
                        "opencode (configuration required)".into()
                    }
                    _ => "opencode (not installed)".into(),
                },
                version: None,
                diagnostic,
                binary: Some(binary),
                readiness,
                models: Vec::new(),
            },
        }
    }

    pub(super) fn checked_binary(path: &Path) -> Result<String, AppError> {
        probe(path)
            .map(|(version, _)| version)
            .map_err(|(_, detail)| AppError::Other(detail))
    }
}

fn probe(path: &Path) -> Result<(String, Vec<String>), (OpenCodeReadiness, String)> {
    let version_output = run(path, &["--version"])
        .map_err(|detail| (OpenCodeReadiness::InvalidInstallation, detail))?;
    let version = parse_version(&version_output).ok_or_else(|| {
        (
            OpenCodeReadiness::InvalidInstallation,
            "OpenCode did not report a parseable version".to_owned(),
        )
    })?;
    let model_output = run(path, &["--pure", "models"]).map_err(|detail| {
        let lowered = detail.to_ascii_lowercase();
        let readiness = if [
            "authentication",
            "not authenticated",
            "api key",
            "unauthorized",
        ]
        .iter()
        .any(|marker| lowered.contains(marker))
        {
            OpenCodeReadiness::AuthenticationRequired
        } else {
            OpenCodeReadiness::InvalidInstallation
        };
        (
            readiness,
            format!("OpenCode could not list configured models: {detail}"),
        )
    })?;
    let mut models = model_output
        .lines()
        .map(str::trim)
        .filter(|line| line.contains('/') && !line.starts_with('/') && !line.ends_with('/'))
        .map(str::to_owned)
        .collect::<Vec<_>>();
    models.sort();
    models.dedup();
    if models.is_empty() {
        return Err((
            OpenCodeReadiness::ConfigurationRequired,
            "OpenCode has no configured models. Configure a provider with `opencode auth login` or set up a local model, then rediscover.".into(),
        ));
    }
    Ok((version, models))
}

fn run(binary: &Path, args: &[&str]) -> Result<String, String> {
    let mut argv = vec![binary.to_string_lossy().into_owned()];
    argv.extend(args.iter().map(|arg| (*arg).to_owned()));
    let task = crate::harness::pi_proc::spawn(&argv, Path::new("."))
        .map_err(|error| format!("OpenCode could not start: {error}"))?;
    let deadline = Instant::now() + PROBE_TIMEOUT;
    let mut stdout = String::new();
    let mut stderr = String::new();
    let mut exit = None;
    while Instant::now() < deadline {
        match task.poll_next(Duration::from_millis(100)) {
            Ok(StreamEvt::Stdout(line)) => append_limited(&mut stdout, &line),
            Ok(StreamEvt::Stderr(line)) => append_limited(&mut stderr, &line),
            Ok(StreamEvt::Exited(success)) => exit = Some(success),
            Err(PollState::Closed) => break,
            Err(PollState::Pending) => {}
        }
    }
    if exit == Some(true) {
        Ok(stdout)
    } else {
        task.kill();
        let _ = task.settle(Duration::from_secs(1));
        let detail = stderr.trim();
        Err(if detail.is_empty() {
            "OpenCode readiness command failed or timed out".into()
        } else {
            format!("OpenCode readiness command failed: {detail}")
        })
    }
}

fn parse_version(text: &str) -> Option<String> {
    let start = text.find(|ch: char| ch.is_ascii_digit())?;
    let version = text[start..]
        .split_whitespace()
        .next()?
        .trim_start_matches('v');
    let mut parts = version.split('.');
    for _ in 0..3 {
        parts.next()?.parse::<u64>().ok()?;
    }
    Some(version.to_owned())
}

fn append_limited(target: &mut String, line: &str) {
    const LIMIT: usize = 32_000;
    let current_len = target.chars().count();
    if current_len >= LIMIT {
        return;
    }
    let remaining = LIMIT - current_len;
    target.push_str(&line.chars().take(remaining).collect::<String>());
    target.push('\n');
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

//! Restricted, headless Claude Code adapter for isolated Kool.ad/e task work.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use crate::error::AppError;
use crate::harness::pi_proc::{PollState, StreamEvt};

pub const CLAUDE_BINARY_ENV: &str = "KOOLADE_CLAUDE_BIN";
pub const CLAUDE_MODEL_ENV: &str = "KOOLADE_CLAUDE_MODEL";
const PROBE_TIMEOUT: Duration = Duration::from_secs(10);
const MINIMUM_VERSION: (u64, u64, u64) = (2, 1, 268);

mod execute;
mod stream;
#[cfg(test)]
pub(super) use stream::{ClaudeEvent, command, parse_event, prompt_input};

#[derive(Default)]
pub struct ClaudeHarness;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClaudeReadiness {
    Missing,
    Unsupported,
    AuthenticationRequired,
    Ready,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClaudeProbeReport {
    pub status: String,
    pub diagnostic: String,
    pub binary: Option<PathBuf>,
    pub readiness: ClaudeReadiness,
}

impl ClaudeHarness {
    pub fn locate_binary() -> Result<PathBuf, AppError> {
        if let Ok(value) = std::env::var(CLAUDE_BINARY_ENV)
            && !value.trim().is_empty()
        {
            let path = PathBuf::from(value);
            if executable(&path) {
                return Ok(path);
            }
            return Err(AppError::HarnessNotFound {
                detail: format!("{CLAUDE_BINARY_ENV} does not name an executable file"),
            });
        }
        if let Some(paths) = std::env::var_os("PATH") {
            for directory in std::env::split_paths(&paths) {
                let candidate = directory.join("claude");
                if executable(&candidate) {
                    return Ok(candidate);
                }
            }
        }
        if let Some(home) = std::env::var_os("HOME") {
            for site in [".npm-global/bin", ".local/bin"] {
                let candidate = PathBuf::from(&home).join(site).join("claude");
                if executable(&candidate) {
                    return Ok(candidate);
                }
            }
        }
        Err(AppError::HarnessNotFound {
            detail: format!(
                "claude executable not found in PATH or common install locations; set {CLAUDE_BINARY_ENV} to override"
            ),
        })
    }

    pub fn probe_report() -> ClaudeProbeReport {
        let binary = match Self::locate_binary() {
            Ok(binary) => binary,
            Err(error) => {
                return ClaudeProbeReport {
                    status: "claude (not installed)".into(),
                    diagnostic: error.detail(),
                    binary: None,
                    readiness: ClaudeReadiness::Missing,
                };
            }
        };
        match probe(&binary) {
            Ok(version) => ClaudeProbeReport {
                status: format!("claude {version}"),
                diagnostic: String::new(),
                binary: Some(binary),
                readiness: ClaudeReadiness::Ready,
            },
            Err((readiness, diagnostic)) => ClaudeProbeReport {
                status: match readiness {
                    ClaudeReadiness::Unsupported => "claude (update required)".into(),
                    ClaudeReadiness::AuthenticationRequired => {
                        "claude (authentication required)".into()
                    }
                    _ => "claude (unavailable)".into(),
                },
                diagnostic,
                binary: Some(binary),
                readiness,
            },
        }
    }

    pub(super) fn checked_binary(path: &Path) -> Result<String, AppError> {
        probe(path).map_err(|(_, detail)| AppError::Other(detail))
    }
}

fn probe(path: &Path) -> Result<String, (ClaudeReadiness, String)> {
    let version_output =
        run(path, &["--version"]).map_err(|error| (ClaudeReadiness::Unsupported, error))?;
    let version = version_output.lines().next().unwrap_or_default().trim();
    let parsed = parse_version(version).ok_or_else(|| {
        (
            ClaudeReadiness::Unsupported,
            "Claude Code did not report a parseable version".to_owned(),
        )
    })?;
    if parsed < MINIMUM_VERSION {
        return Err((
            ClaudeReadiness::Unsupported,
            format!(
                "Claude Code {version} is too old for restricted unattended mode; update to 2.1.268 or later"
            ),
        ));
    }
    let auth = run(path, &["auth", "status"]);
    match auth {
        Ok(output)
            if serde_json::from_str::<serde_json::Value>(&output)
                .ok()
                .and_then(|value| value["authMethod"].as_str().map(str::to_owned))
                .is_some_and(|method| method != "none") =>
        {
            Ok(version.to_owned())
        }
        Ok(_) | Err(_) => Err((
            ClaudeReadiness::AuthenticationRequired,
            "Claude Code is installed but is not authenticated. Run `claude auth login` and retry."
                .into(),
        )),
    }
}

fn run(binary: &Path, args: &[&str]) -> Result<String, String> {
    let mut argv = vec![binary.to_string_lossy().into_owned()];
    argv.extend(args.iter().map(|arg| (*arg).to_owned()));
    let task = crate::harness::pi_proc::spawn(&argv, Path::new("."))
        .map_err(|error| format!("Claude Code could not start: {error}"))?;
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
            Ok(StreamEvt::Exited(success)) => {
                exit = Some(success);
            }
            Err(PollState::Closed) => break,
            Err(PollState::Pending) => {}
        }
    }
    if exit == Some(true) {
        Ok(stdout)
    } else {
        task.kill();
        let _ = task.settle(Duration::from_secs(1));
        Err("Claude Code readiness command failed or timed out".into())
    }
}

fn parse_version(text: &str) -> Option<(u64, u64, u64)> {
    let start = text.find(|ch: char| ch.is_ascii_digit())?;
    let version = text[start..].split_whitespace().next()?;
    let mut components = version.split('.').map(str::parse::<u64>);
    Some((
        components.next()?.ok()?,
        components.next()?.ok()?,
        components.next()?.ok()?,
    ))
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
#[path = "claude_harness/tests.rs"]
mod tests;

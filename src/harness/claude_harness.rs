//! Restricted, headless Claude Code adapter for isolated Kool.ad/e task work.

use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::error::AppError;

pub const CLAUDE_BINARY_ENV: &str = "KOOLADE_CLAUDE_BIN";
pub const CLAUDE_MODEL_ENV: &str = "KOOLADE_CLAUDE_MODEL";
const PROBE_TIMEOUT: Duration = Duration::from_secs(10);
const MINIMUM_VERSION: (u64, u64, u64) = (2, 1, 268);

mod execute;
mod stream;
#[cfg(test)]
pub(super) use execute::developer_instructions;
#[cfg(test)]
pub(super) use stream::{ClaudeEvent, command, parse_event, prompt_input};

#[derive(Default)]
pub struct ClaudeHarness;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClaudeReadiness {
    Missing,
    Unsupported,
    Unusable,
    AuthenticationRequired,
    Ready,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClaudeProbeReport {
    pub status: String,
    pub version: Option<String>,
    pub diagnostic: String,
    pub binary: Option<PathBuf>,
    pub readiness: ClaudeReadiness,
}

impl ClaudeHarness {
    pub fn locate_binary() -> Result<PathBuf, AppError> {
        if let Some(path) = crate::harness::manual_executable_path("claude")? {
            return Ok(path);
        }
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
        for candidate in crate::harness::discovery::host_executable_candidates("claude") {
            if executable(&candidate) {
                return Ok(candidate);
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
                    version: None,
                    diagnostic: error.detail(),
                    binary: None,
                    readiness: ClaudeReadiness::Missing,
                };
            }
        };
        match probe(&binary) {
            Ok(version) => ClaudeProbeReport {
                status: format!("claude {version}"),
                version: Some(version),
                diagnostic: String::new(),
                binary: Some(binary),
                readiness: ClaudeReadiness::Ready,
            },
            Err((readiness, diagnostic, version)) => ClaudeProbeReport {
                status: match readiness {
                    ClaudeReadiness::Unsupported => "claude (update required)".into(),
                    ClaudeReadiness::AuthenticationRequired => {
                        "claude (authentication required)".into()
                    }
                    _ => "claude (unavailable)".into(),
                },
                version,
                diagnostic,
                binary: Some(binary),
                readiness,
            },
        }
    }

    pub(super) fn checked_binary(path: &Path) -> Result<String, AppError> {
        probe(path).map_err(|(_, detail, _)| AppError::Other(detail))
    }
}

fn probe(path: &Path) -> Result<String, (ClaudeReadiness, String, Option<String>)> {
    let (version_output, version_success) =
        run(path, &["--version"]).map_err(|error| (ClaudeReadiness::Unusable, error, None))?;
    if !version_success {
        return Err((
            ClaudeReadiness::Unusable,
            "Claude Code version command failed".into(),
            None,
        ));
    }
    let version =
        crate::harness::discovery::normalize_cli_version(&version_output, &["Claude Code"], true)
            .ok_or_else(|| {
            (
                ClaudeReadiness::Unsupported,
                "Claude Code did not report a parseable version".to_owned(),
                None,
            )
        })?;
    let parsed = parse_version(&version).expect("normalized version has three numeric fields");
    if parsed < MINIMUM_VERSION {
        return Err((
            ClaudeReadiness::Unsupported,
            format!(
                "Claude Code {version} is too old for restricted unattended mode; update to 2.1.268 or later"
            ),
            Some(version.clone()),
        ));
    }
    crate::harness::execution_security::ProfileProbeEvidence::probe(
        crate::harness::execution_security::CliProvider::Claude,
        path,
        version.clone(),
    )
    .map_err(|error| {
        (
            ClaudeReadiness::Unsupported,
            error.to_string(),
            Some(version.clone()),
        )
    })?;
    let auth = run(path, &["auth", "status"]);
    match auth {
        Ok((output, true))
            if serde_json::from_str::<serde_json::Value>(&output)
                .ok()
                .and_then(|value| value["authMethod"].as_str().map(str::to_owned))
                .is_some_and(is_supported_auth_method) =>
        {
            Ok(version)
        }
        Ok((output, _)) if serde_json::from_str::<serde_json::Value>(&output)
            .ok()
            .and_then(|value| value["authMethod"].as_str().map(str::to_owned))
            .is_some_and(|method| method == "none") => Err((
            ClaudeReadiness::AuthenticationRequired,
            "Claude Code is installed but is not authenticated. Run `claude auth login` and retry."
                .into(),
            Some(version.to_owned()),
        )),
        Ok((_, _)) => Err((
            ClaudeReadiness::Unusable,
            "Claude Code auth status returned an unrecognized response.".into(),
            Some(version.to_owned()),
        )),
        Err(error) => Err((
            ClaudeReadiness::Unusable,
            format!("Claude Code auth status could not be checked: {error}"),
            Some(version.to_owned()),
        )),
    }
}

fn run(binary: &Path, args: &[&str]) -> Result<(String, bool), String> {
    let argv = std::iter::once(binary.to_string_lossy().into_owned())
        .chain(args.iter().map(|arg| (*arg).to_owned()))
        .collect::<Vec<_>>();
    let output = crate::harness::discovery::run(
        &argv,
        PROBE_TIMEOUT,
        crate::harness::discovery::MAX_PROBE_OUTPUT_BYTES,
        crate::harness::discovery::MAX_PROBE_OUTPUT_BYTES,
        crate::harness::execution_security::CliProvider::Claude.excluded_child_environment(),
    )
    .map_err(|error| format!("Claude Code readiness command failed: {error}"))?;
    if output.stdout_truncated || output.stderr_truncated {
        return Err("Claude Code readiness output exceeded the size limit".into());
    }
    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
    let stderr = String::from_utf8_lossy(&output.stderr);
    Ok((format!("{stdout}\n{stderr}"), output.success))
}

fn is_supported_auth_method(method: String) -> bool {
    matches!(
        method.trim().to_ascii_lowercase().as_str(),
        "claude.ai" | "console" | "oauth" | "apikey" | "api_key" | "bedrock" | "vertex"
    )
}

fn parse_version(text: &str) -> Option<(u64, u64, u64)> {
    let version = crate::harness::discovery::normalize_cli_version(text, &["Claude Code"], true)?;
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

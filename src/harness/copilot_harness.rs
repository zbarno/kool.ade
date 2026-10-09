//! GitHub Copilot CLI adapter for task repositories.

use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::error::AppError;

pub const COPILOT_BINARY_ENV: &str = "KOOLADE_COPILOT_BIN";
pub const COPILOT_MODEL_ENV: &str = "KOOLADE_COPILOT_MODEL";
mod execute;
#[cfg(test)]
mod tests;

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
        if let Some(paths) = std::env::var_os("PATH") {
            for directory in std::env::split_paths(&paths) {
                let candidate = directory.join("copilot");
                if executable(&candidate) {
                    return Ok(candidate);
                }
            }
        }
        if let Some(home) = std::env::var_os("HOME") {
            for site in [".npm-global/bin", ".local/bin"] {
                let candidate = PathBuf::from(&home).join(site).join("copilot");
                if executable(&candidate) {
                    return Ok(candidate);
                }
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
    output.lines().map(str::trim).find_map(|line| {
        let lower = line.to_ascii_lowercase();
        let identity_tokens = lower
            .split(|character: char| !character.is_ascii_alphanumeric())
            .filter(|token| !token.is_empty())
            .collect::<Vec<_>>();
        let recognized_identity = identity_tokens
            .windows(2)
            .any(|pair| pair == ["github", "copilot"] || pair == ["copilot", "cli"]);
        let has_version = line.split_whitespace().any(|word| {
            let token = word
                .trim_matches(|character: char| {
                    !character.is_ascii_alphanumeric() && character != '.'
                })
                .trim_matches('.');
            let token = token
                .strip_prefix('v')
                .or_else(|| token.strip_prefix('V'))
                .unwrap_or(token);
            let pieces = token.split('.').collect::<Vec<_>>();
            pieces.len() >= 2
                && pieces
                    .iter()
                    .all(|piece| !piece.is_empty() && piece.chars().all(|c| c.is_ascii_digit()))
        });
        (recognized_identity && has_version).then(|| line.to_owned())
    })
}

fn executable(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    path.is_file()
        && std::fs::metadata(path).is_ok_and(|metadata| metadata.permissions().mode() & 0o111 != 0)
}

fn run(binary: &Path, args: &[&str]) -> Result<(String, bool), String> {
    let argv = std::iter::once(binary.to_string_lossy().into_owned())
        .chain(args.iter().map(|arg| (*arg).to_owned()))
        .collect::<Vec<_>>();
    let task = crate::harness::pi_proc::spawn_with_input_env_excluding(
        &argv,
        Path::new("."),
        None,
        &[],
        crate::harness::execution_security::CliProvider::Copilot.excluded_child_environment(),
    )
    .map_err(|error| format!("Could not start GitHub Copilot CLI: {error}"))?;
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    let mut output = Vec::new();
    let mut status = None;
    loop {
        if std::time::Instant::now() >= deadline {
            task.kill();
            let _ = task.settle(Duration::from_secs(2));
            return Err("GitHub Copilot CLI readiness command timed out".into());
        }
        match task.poll_next(Duration::from_millis(100)) {
            Ok(crate::harness::pi_proc::StreamEvt::Stdout(line)) => output.push(line),
            Ok(crate::harness::pi_proc::StreamEvt::Stderr(_)) => {}
            Ok(crate::harness::pi_proc::StreamEvt::Exited(success)) => status = Some(success),
            Err(crate::harness::pi_proc::PollState::Closed) => break,
            Err(crate::harness::pi_proc::PollState::Pending) => {}
        }
    }
    Ok((output.join("\n"), status == Some(true)))
}

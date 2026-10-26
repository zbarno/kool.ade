use std::{path::Path, time::Duration};

use crate::error::AppError;

use super::{CodexHarness, CodexProbeReport, CodexReadiness};

const OPENAI_API_KEY_ENV: &str = "OPENAI_API_KEY";
const PROBE_TIMEOUT: Duration = Duration::from_secs(10);

impl CodexHarness {
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
        match check_binary_readiness(&path) {
            Ok((version, readiness)) => {
                let (status, diagnostic, ready) = match readiness {
                    CodexReadiness::Ready => (format!("codex {version}"), String::new(), true),
                    CodexReadiness::AuthenticationRequired => (
                        "codex (authentication required)".into(),
                        "Codex CLI is installed but is not authenticated. Run `codex login` and retry."
                            .into(),
                        false,
                    ),
                    CodexReadiness::AuthenticationUnverified => (
                        "codex (authentication unverified)".into(),
                        "Codex account and model access remain unverified; no generative model call was made."
                            .into(),
                        false,
                    ),
                    CodexReadiness::Missing | CodexReadiness::Unusable => (
                        "codex (installed but unusable)".into(),
                        "Codex CLI readiness could not be verified.".into(),
                        false,
                    ),
                };
                CodexProbeReport {
                    status,
                    version: Some(version),
                    diagnostic,
                    binary: Some(path),
                    ready,
                    readiness,
                }
            }
            Err(error) => CodexProbeReport {
                status: "codex (installed but unusable)".into(),
                version,
                diagnostic: error.detail(),
                binary: Some(path),
                ready: false,
                readiness: CodexReadiness::Unusable,
            },
        }
    }

    pub(super) fn check_binary(path: &Path) -> Result<String, AppError> {
        let (version, readiness) = check_binary_readiness(path)?;
        match readiness {
            CodexReadiness::Ready => Ok(version),
            CodexReadiness::AuthenticationRequired => Err(AppError::Other(
                "Codex CLI is installed but is not authenticated. Run `codex login` and retry."
                    .into(),
            )),
            CodexReadiness::AuthenticationUnverified => {
                // The test-only override exercises fake CLI lifecycle paths; it is
                // not evidence for discovery and is absent from production builds.
                #[cfg(test)]
                if crate::harness::uncontained_provider_test_execution_enabled() {
                    return Ok(version);
                }
                Err(AppError::Other(
                    "Codex account and model access remain unverified; execution is blocked."
                        .into(),
                ))
            }
            CodexReadiness::Missing | CodexReadiness::Unusable => Err(AppError::Other(
                "Codex CLI readiness could not be verified.".into(),
            )),
        }
    }
}

fn check_binary_readiness(path: &Path) -> Result<(String, CodexReadiness), AppError> {
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

    if workload_identity_selected() || api_key_environment_present() {
        return Ok((version, CodexReadiness::AuthenticationUnverified));
    }

    let (output, success) = run_probe_status(path, &["login", "status"])?;
    let output = output.to_ascii_lowercase();
    if reports_api_key_configured(&output) {
        return Ok((version, CodexReadiness::AuthenticationUnverified));
    }
    if output.contains("not logged in")
        || output.contains("logged out")
        || output.contains("not authenticated")
    {
        return Ok((version, CodexReadiness::AuthenticationRequired));
    }
    if output.contains("logged in") {
        return Ok((version, CodexReadiness::AuthenticationUnverified));
    }
    if success {
        return Ok((version, CodexReadiness::AuthenticationUnverified));
    }
    Err(AppError::Other(
        "Codex CLI login status could not be checked because the command failed.".into(),
    ))
}

fn workload_identity_selected() -> bool {
    std::env::var_os("OPENAI_FEDERATION_RULE_ID").is_some()
        || std::env::var_os("OPENAI_IDENTITY_TOKEN_FILE").is_some()
}

fn api_key_environment_present() -> bool {
    std::env::var_os(OPENAI_API_KEY_ENV).is_some()
}

fn reports_api_key_configured(output: &str) -> bool {
    output.contains("api key configured")
        || output.contains("logged in using an api key")
        || output.contains("logged in using api key")
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

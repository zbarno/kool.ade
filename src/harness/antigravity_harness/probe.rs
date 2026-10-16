use std::path::Path;
use std::time::Duration;

use super::parse_models;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct AntigravityProbeOutcome {
    pub(super) version: String,
    pub(super) models: Vec<String>,
    pub(super) catalog_diagnostic: Option<String>,
    pub(super) configuration_required: bool,
}

pub(super) fn probe(
    binary: &Path,
    timeout: Duration,
) -> Result<AntigravityProbeOutcome, (String, Option<String>)> {
    probe_with_timeouts(binary, timeout, timeout)
}

pub(super) fn probe_with_timeouts(
    binary: &Path,
    version_timeout: Duration,
    catalog_timeout: Duration,
) -> Result<AntigravityProbeOutcome, (String, Option<String>)> {
    let version = probe_version(binary, version_timeout)?;
    crate::harness::execution_security::ProfileProbeEvidence::probe(
        crate::harness::execution_security::CliProvider::Antigravity,
        binary,
        version.clone(),
    )
    .map_err(|error| (error.to_string(), Some(version.clone())))?;
    let (models, catalog_diagnostic, configuration_required) =
        match run(binary, &["models"], catalog_timeout) {
            Ok((output, _, true)) => {
                let models = parse_models(&output);
                if models.is_empty() {
                    (
                        models,
                        Some("Model catalog unavailable: Antigravity returned no models.".into()),
                        false,
                    )
                } else {
                    (models, None, false)
                }
            }
            Ok((_, stderr, false)) if authentication_required(&stderr) => (
                Vec::new(),
                Some(
                    "Model catalog unavailable because Antigravity authentication is required."
                        .into(),
                ),
                true,
            ),
            Ok((_, _, false)) => (
                Vec::new(),
                Some("Model catalog unavailable: Antigravity model discovery failed.".into()),
                false,
            ),
            Err((detail, _)) => (
                Vec::new(),
                Some(format!("Model catalog unavailable: {detail}")),
                false,
            ),
        };
    Ok(AntigravityProbeOutcome {
        version,
        models,
        catalog_diagnostic,
        configuration_required,
    })
}

pub(super) fn probe_version(
    binary: &Path,
    timeout: Duration,
) -> Result<String, (String, Option<String>)> {
    let (output, _, success) = run(binary, &["--version"], timeout)?;
    if !success {
        return Err(("Antigravity version command failed".into(), None));
    }
    let version = output.lines().next().unwrap_or_default().trim().to_owned();
    if version.is_empty() {
        return Err(("Antigravity did not report a version".into(), None));
    }
    Ok(version)
}

fn run(
    binary: &Path,
    args: &[&str],
    timeout: Duration,
) -> Result<(String, String, bool), (String, Option<String>)> {
    let mut argv = vec![binary.to_string_lossy().into_owned()];
    argv.extend(args.iter().map(|arg| (*arg).to_owned()));
    let output = crate::harness::discovery::run(
        &argv,
        timeout,
        crate::harness::discovery::MAX_PROBE_OUTPUT_BYTES,
        crate::harness::discovery::MAX_PROBE_OUTPUT_BYTES,
        crate::harness::execution_security::CliProvider::Antigravity.excluded_child_environment(),
    )
    .map_err(|error| {
        (
            format!("Antigravity CLI readiness check failed: {error}"),
            None,
        )
    })?;
    if output.stdout_truncated || output.stderr_truncated {
        return Err((
            "Antigravity CLI readiness output exceeded the size limit".into(),
            None,
        ));
    }
    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
    Ok((stdout, stderr, output.success))
}

fn authentication_required(diagnostic: &str) -> bool {
    let diagnostic = diagnostic.to_ascii_lowercase();
    [
        "authentication required",
        "not authenticated",
        "api key",
        "unauthorized",
        "login required",
        "sign in",
    ]
    .iter()
    .any(|marker| diagnostic.contains(marker))
}

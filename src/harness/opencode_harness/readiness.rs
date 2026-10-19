use std::path::Path;
use std::time::Duration;

use super::OpenCodeReadiness;

#[derive(Debug, PartialEq, Eq)]
pub(super) struct OpenCodeProbeOutcome {
    pub(super) version: String,
    pub(super) models: Vec<String>,
    pub(super) readiness: OpenCodeReadiness,
    pub(super) diagnostic: String,
}

pub(super) fn probe(
    path: &Path,
    timeout: Duration,
) -> Result<OpenCodeProbeOutcome, (OpenCodeReadiness, String, Option<String>)> {
    probe_with_timeouts(path, timeout, timeout)
}

pub(super) fn probe_with_timeouts(
    path: &Path,
    version_timeout: Duration,
    catalog_timeout: Duration,
) -> Result<OpenCodeProbeOutcome, (OpenCodeReadiness, String, Option<String>)> {
    let version = probe_version(path, version_timeout)
        .map_err(|detail| (OpenCodeReadiness::InvalidInstallation, detail, None))?;
    crate::harness::execution_security::ProfileProbeEvidence::probe(
        crate::harness::execution_security::CliProvider::OpenCode,
        path,
        version.clone(),
    )
    .map_err(|error| {
        (
            OpenCodeReadiness::InvalidInstallation,
            error.to_string(),
            Some(version.clone()),
        )
    })?;

    let model_output = match run(path, &["--pure", "models"], catalog_timeout) {
        Ok(output) => output,
        Err(detail) => {
            return Ok(OpenCodeProbeOutcome {
                version,
                models: Vec::new(),
                readiness: OpenCodeReadiness::CatalogUnavailable,
                diagnostic: format!("Model catalog unavailable: {detail}"),
            });
        }
    };
    let models = parse_models(&model_output);
    if models.is_empty() {
        return Ok(OpenCodeProbeOutcome {
            version,
            models,
            readiness: OpenCodeReadiness::ConfigurationRequired,
            diagnostic: "OpenCode has no configured models. Configure a provider with `opencode auth login` or set up a local model, then rediscover.".into(),
        });
    }
    Ok(OpenCodeProbeOutcome {
        version,
        models,
        readiness: OpenCodeReadiness::Ready,
        diagnostic: String::new(),
    })
}

pub(super) fn parse_models(output: &str) -> Vec<String> {
    let mut models = output
        .lines()
        .map(str::trim)
        .filter(|line| {
            line.contains('/') && crate::harness::discovery::is_valid_model_identifier(line)
        })
        .map(str::to_owned)
        .collect::<Vec<_>>();
    models.sort();
    models.dedup();
    models
}

pub(super) fn probe_version(path: &Path, timeout: Duration) -> Result<String, String> {
    let output = run(path, &["--version"], timeout)?;
    parse_version(&output).ok_or_else(|| "OpenCode did not report a parseable version".into())
}

fn run(binary: &Path, args: &[&str], timeout: Duration) -> Result<String, String> {
    let argv = std::iter::once(binary.to_string_lossy().into_owned())
        .chain(args.iter().map(|arg| (*arg).to_owned()))
        .collect::<Vec<_>>();
    let output = crate::harness::discovery::run(
        &argv,
        timeout,
        crate::harness::discovery::MAX_PROBE_OUTPUT_BYTES,
        crate::harness::discovery::MAX_PROBE_OUTPUT_BYTES,
        crate::harness::execution_security::CliProvider::OpenCode.excluded_child_environment(),
    )
    .map_err(|error| format!("OpenCode readiness command failed: {error}"))?;
    if output.stdout_truncated || output.stderr_truncated {
        return Err("OpenCode readiness output exceeded the size limit".into());
    }
    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
    if output.success {
        Ok(stdout)
    } else {
        Err(if args.contains(&"models") {
            "OpenCode model discovery failed. Check provider configuration and retry.".into()
        } else {
            "OpenCode version check failed. Verify the installation and retry.".into()
        })
    }
}

pub(super) fn parse_version(text: &str) -> Option<String> {
    crate::harness::discovery::normalize_cli_version(text, &["opencode"], true)
}
